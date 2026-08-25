//! `GranularSimulation` — the Phase 1 module entry point.
//!
//! Owns its GPU buffers/pipeline; receives the shared [`GpuContext`] by
//! reference at construction (ADR-0002). `step()` is deliberately blocking
//! and follows the staging-buffer readback pattern of ADR-0006.

use crate::core::rng::Rng;
use crate::granular::config::{GranularConfig, ResolvedMode};
use crate::granular::placement::place_grains;
use crate::granular::types::{GpuParams, GranularData, ModeEntry};
use crate::{Error, GpuError, Result};
use bytemuck::cast_slice;
use std::sync::mpsc;
use wgpu::util::DeviceExt;

const WORKGROUP_SIZE: u32 = 64;

/// Chladni-plate granular simulation (contract: docs/CONTRACT.md § Granular).
pub struct GranularSimulation {
    device: wgpu::Device,
    queue: wgpu::Queue,

    pipeline: wgpu::ComputePipeline,
    bind_group: wgpu::BindGroup,

    grains_buf: wgpu::Buffer,
    modes_buf: wgpu::Buffer,
    params_buf: wgpu::Buffer,
    staging_buf: wgpu::Buffer,

    count: u32,
    side: f32,
    config: GranularConfig,
    /// Simulation time accumulated since construction (seconds).
    time: f32,
}

impl GranularSimulation {
    /// Validates the configuration eagerly (contract F1) and builds every
    /// GPU resource once — later setters never reallocate (invariant I1).
    pub fn new(ctx: &crate::GpuContext, config: GranularConfig) -> Result<Self> {
        config.validate()?;

        let device = ctx.device().clone();
        let queue = ctx.queue().clone();

        let count = config.grains.count;
        let side = match config.medium {
            crate::granular::PlateSpec::Idealized { side } => side,
        };

        // ---- Initial grain state (identical seed path as reference) ----
        let mut rng = Rng::new(config.grains.seed);
        let initial: Vec<GranularData> = place_grains(&config, &mut rng, side as f64)
            .into_iter()
            .map(|(x, y)| GranularData {
                position: [x as f32, y as f32],
                velocity: [0.0; 2],
            })
            .collect();

        // ---- Mode table (non-empty for all three sources) ----
        let entries = mode_entries(config.resolve_modes());

        // ---- Buffers (allocated once — I1) ----
        let grains_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cymatrox.granular.grains"),
            contents: cast_slice(&initial),
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
        });
        let modes_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cymatrox.granular.modes"),
            contents: cast_slice(&entries),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        });
        let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cymatrox.granular.params"),
            contents: &[0u8; std::mem::size_of::<GpuParams>()],
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let staging_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cymatrox.granular.staging"),
            size: count as u64 * std::mem::size_of::<GranularData>() as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // ---- Pipeline ----
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cymatrox.granular.shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("granular.wgsl").into()),
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cymatrox.granular.bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(
                            std::mem::size_of::<GpuParams>() as u64
                        ),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(
                            std::mem::size_of::<GranularData>() as u64,
                        ),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(
                            std::mem::size_of::<ModeEntry>() as u64
                        ),
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cymatrox.granular.pl"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("cymatrox.granular.pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cymatrox.granular.bg"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: grains_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: modes_buf.as_entire_binding(),
                },
            ],
        });

        Ok(Self {
            device,
            queue,
            pipeline,
            bind_group,
            grains_buf,
            modes_buf,
            params_buf,
            staging_buf,
            count,
            side,
            config,
            time: 0.0,
        })
    }

    /// Live retune of the driving frequency — uniform rewrite only (I1).
    ///
    /// Takes effect on the next [`step`](Self::step); with `Auto`/`Measured`
    /// mode sources this also re-selects the mode per ADR-0009 semantics.
    pub fn set_frequency(&mut self, frequency_hz: f32) {
        self.config.experiment.frequency_hz = frequency_hz;
    }

    /// Live amplitude change — uniform rewrite only (I1).
    pub fn set_amplitude(&mut self, amplitude: f32) {
        self.config.experiment.amplitude = amplitude;
    }

    /// Advances the simulation by one `dt` and returns the post-step state.
    ///
    /// Blocking by design ([ADR-0006](../../docs/adr/0006-gpu-cpu-readback-strategy.md)):
    /// dispatch → storage→staging copy → submit → poll(Wait) → map → `Vec`.
    pub fn step(&mut self) -> Result<Vec<GranularData>> {
        // Re-resolve modes each step so Auto/Measured retuning is live.
        let entries = mode_entries(self.config.resolve_modes());
        self.queue
            .write_buffer(&self.modes_buf, 0, cast_slice(&entries));

        let s = self.config.solver;
        let params = GpuParams {
            plate_size: self.side,
            frequency_hz: self.config.experiment.frequency_hz,
            amplitude: self.config.experiment.amplitude,
            dt: s.dt,
            drag: s.drag,
            restitution: s.restitution,
            coupling_k: s.coupling_k,
            time: self.time,
        };
        self.queue
            .write_buffer(&self.params_buf, 0, bytemuck::bytes_of(&params));

        let byte_len = self.count as u64 * std::mem::size_of::<GranularData>() as u64;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("cymatrox.granular.step"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("cymatrox.granular.pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.dispatch_workgroups(self.count.div_ceil(WORKGROUP_SIZE), 1, 1);
        }
        // ADR-0006: storage buffers are not CPU-mappable — hop through the
        // staging buffer.
        encoder.copy_buffer_to_buffer(&self.grains_buf, 0, &self.staging_buf, 0, byte_len);
        self.queue.submit(Some(encoder.finish()));

        let out = self.read_back(byte_len)?;

        self.time += s.dt;
        Ok(out)
    }

    /// Non-blocking variant of [`step`](Self::step) for WASM and async contexts.
    ///
    /// Same GPU pipeline as `step()` but uses `futures_channel::oneshot` for
    /// the mapping callback, and skips `device.poll(Wait)` on WASM where the
    /// browser's event loop drives completion.
    #[cfg(feature = "web")]
    pub async fn step_async(&mut self) -> Result<Vec<GranularData>> {
        let entries = mode_entries(self.config.resolve_modes());
        self.queue
            .write_buffer(&self.modes_buf, 0, cast_slice(&entries));

        let s = self.config.solver;
        let params = GpuParams {
            plate_size: self.side,
            frequency_hz: self.config.experiment.frequency_hz,
            amplitude: self.config.experiment.amplitude,
            dt: s.dt,
            drag: s.drag,
            restitution: s.restitution,
            coupling_k: s.coupling_k,
            time: self.time,
        };
        self.queue
            .write_buffer(&self.params_buf, 0, bytemuck::bytes_of(&params));

        let byte_len = self.count as u64 * std::mem::size_of::<GranularData>() as u64;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("cymatrox.granular.step_async"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("cymatrox.granular.pass_async"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.dispatch_workgroups(self.count.div_ceil(WORKGROUP_SIZE), 1, 1);
        }
        encoder.copy_buffer_to_buffer(&self.grains_buf, 0, &self.staging_buf, 0, byte_len);
        self.queue.submit(Some(encoder.finish()));

        let out = crate::core::readback::read_back_async::<GranularData>(
            &self.staging_buf,
            byte_len,
            &self.device,
        )
        .await?;

        self.time += s.dt;
        debug_assert_eq!(out.len(), self.count as usize);
        Ok(out)
    }

    /// ADR-0006 readback: map the staging buffer after a blocking poll,
    /// copy the bytes into a `Vec<GranularData>`, unmap.
    fn read_back(&self, byte_len: u64) -> Result<Vec<GranularData>> {
        let (tx, rx) = mpsc::channel();
        self.staging_buf
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });

        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| Error::Gpu(GpuError::Readback(format!("device poll failed: {e}"))))?;

        match rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(Error::Gpu(GpuError::Readback(e.to_string()))),
            Err(_) => {
                return Err(Error::Gpu(GpuError::Readback(
                    "mapping callback channel closed unexpectedly".into(),
                )));
            }
        }

        let out: Vec<GranularData> = {
            let view = self
                .staging_buf
                .slice(..byte_len)
                .get_mapped_range()
                .map_err(|e| {
                    Error::Gpu(GpuError::Readback(format!("mapped range unavailable: {e}")))
                })?;
            let bytes: &[u8] = &view;
            cast_slice(bytes).to_vec()
        };
        self.staging_buf.unmap();

        debug_assert_eq!(out.len(), self.count as usize);
        Ok(out)
    }
}

fn mode_entries(modes: Vec<ResolvedMode>) -> Vec<ModeEntry> {
    modes
        .into_iter()
        .map(|m| ModeEntry {
            m: m.m,
            n: m.n,
            omega_hz: m.omega_hz,
            _pad: 0,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::granular::config::{
        Driving, GrainBed, InitialDistribution, ModeSelection, PlateSpec, SolverParams,
    };

    fn reference_config(seed: u64) -> GranularConfig {
        GranularConfig {
            experiment: Driving {
                frequency_hz: 440.0,
                amplitude: 1e-4,
                modes: ModeSelection::Auto,
            },
            medium: PlateSpec::Idealized { side: 0.5 },
            grains: GrainBed {
                count: 4096,
                distribution: InitialDistribution::Uniform,
                seed,
            },
            solver: SolverParams {
                dt: 1.0 / 480.0,
                drag: 4.0,
                restitution: 0.6,
                coupling_k: 5.0e5,
                base_frequency_hz: 120.0,
            },
        }
    }

    /// Contract O4/I2 — same seed ⇒ bit-identical trajectories.
    #[tokio::test]
    #[ignore = "requires a GPU-capable host (or software backend)"]
    async fn deterministic_same_seed() {
        let ctx = crate::GpuContext::new().await.expect("gpu context");
        let mut a = GranularSimulation::new(&ctx, reference_config(7)).expect("sim a");
        let mut b = GranularSimulation::new(&ctx, reference_config(7)).expect("sim b");
        for _ in 0..10 {
            assert_eq!(a.step().unwrap(), b.step().unwrap());
        }
    }

    /// Golden-file check vs the f64 reference oracle (ADR-0004/0007):
    /// mean positional deviation stays below `1e-3 · side` after 100 steps.
    #[tokio::test]
    #[ignore = "requires a GPU-capable host (or software backend)"]
    async fn golden_file_within_tolerance() {
        use crate::granular::reference::ReferenceSim;

        let cfg = reference_config(123);
        let side = match cfg.medium {
            PlateSpec::Idealized { side } => side,
        };
        let tolerance = 1e-3 * side;

        let ctx = crate::GpuContext::new().await.expect("gpu context");
        let mut gpu = GranularSimulation::new(&ctx, cfg.clone()).expect("gpu sim");
        let mut cpu = ReferenceSim::new(&cfg);

        for i in 0..100 {
            let frame = gpu.step().unwrap();
            let modes = cfg.resolve_modes();
            cpu.step(&modes, cfg.experiment.amplitude);
            if i == 99 {
                let mean_err: f64 = frame
                    .iter()
                    .zip(cpu.positions())
                    .map(|(gp, cp)| {
                        let dx = gp.position[0] as f64 - cp.0;
                        let dy = gp.position[1] as f64 - cp.1;
                        dx.hypot(dy)
                    })
                    .sum::<f64>()
                    / cfg.grains.count as f64;
                assert!(
                    mean_err < tolerance as f64,
                    "mean positional deviation {mean_err} exceeds tolerance {tolerance}"
                );
            }
        }
    }

    /// step_async() must return bit-identical results to step().
    #[cfg(feature = "web")]
    #[tokio::test]
    #[ignore = "requires a GPU-capable host (or software backend)"]
    async fn step_async_matches_step() {
        let cfg = reference_config(42);
        let ctx = crate::GpuContext::new().await.expect("gpu context");

        let mut sim_sync = GranularSimulation::new(&ctx, cfg.clone()).expect("sync sim");
        let mut sim_async = GranularSimulation::new(&ctx, cfg).expect("async sim");

        let frame_sync = sim_sync.step().expect("step sync");
        let frame_async = sim_async.step_async().await.expect("step async");
        assert_eq!(frame_sync, frame_async);
    }
}
