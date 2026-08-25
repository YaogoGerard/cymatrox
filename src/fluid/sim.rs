//! `FluidSimulation` — the Phase 2 module entry point.
//!
//! Owns its GPU buffers/pipeline; receives the shared [`GpuContext`] by
//! reference at construction (ADR-0002). `step()` is deliberately blocking
//! and follows the staging-buffer readback pattern of ADR-0006.
//!
//! Neighbour reads make double buffering mandatory (contract I3): state
//! ping-pongs between two storage buffers through bind-group swap.

use crate::fluid::config::{DomainShape, FluidConfig};
use crate::fluid::initial::initial_state;
use crate::fluid::types::{FluidSurfaceNode, GpuParams};
use crate::{Error, GpuError, Result};
use std::sync::mpsc;
use wgpu::util::DeviceExt;

const WORKGROUP: u32 = 8;

/// Faraday-wave surface simulation on a masked height-field grid
/// (contract: docs/CONTRACT.md § Fluid · model: ADR-0011).
pub struct FluidSimulation {
    device: wgpu::Device,
    queue: wgpu::Queue,

    pipeline: wgpu::ComputePipeline,
    /// Two bind groups swapping the ping-pong roles of the state buffers.
    /// Each retains its referenced storage buffers, so they are not stored
    /// separately.
    bind_groups: [wgpu::BindGroup; 2],
    current: usize,

    out_buf: wgpu::Buffer,
    params_buf: wgpu::Buffer,
    staging_buf: wgpu::Buffer,

    out_count: u32,
    config: FluidConfig,
    /// Simulation time accumulated since construction (seconds).
    time: f32,
}

impl FluidSimulation {
    /// Validates the configuration eagerly (contract F1) and builds every
    /// GPU resource once — later setters never reallocate (invariant I1).
    pub fn new(ctx: &crate::GpuContext, config: FluidConfig) -> Result<Self> {
        config.validate()?;

        let device = ctx.device().clone();
        let queue = ctx.queue().clone();

        // ---- Initial state: flat + seeded noise (shared with oracle) ----
        let initial = initial_state(&config);
        let state_bytes = bytemuck::cast_slice(&initial).to_vec();

        let state_bufs: [wgpu::Buffer; 2] = [
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("cymatrox.fluid.state_a"),
                contents: &state_bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            }),
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("cymatrox.fluid.state_b"),
                contents: &state_bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            }),
        ];

        let (out_w, out_h) = config.output_dims();
        let out_count = out_w * out_h;
        let out_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cymatrox.fluid.out"),
            contents: &[0u8; std::mem::size_of::<FluidSurfaceNode>()].repeat(out_count as usize),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });
        let staging_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cymatrox.fluid.staging"),
            size: out_count as u64 * std::mem::size_of::<FluidSurfaceNode>() as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cymatrox.fluid.params"),
            contents: &[0u8; std::mem::size_of::<GpuParams>()],
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // ---- Pipeline ----
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cymatrox.fluid.shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("fluid.wgsl").into()),
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cymatrox.fluid.bgl"),
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
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(
                            std::mem::size_of::<FluidSurfaceNode>() as u64,
                        ),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(
                            std::mem::size_of::<FluidSurfaceNode>() as u64,
                        ),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(
                            std::mem::size_of::<FluidSurfaceNode>() as u64,
                        ),
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cymatrox.fluid.pl"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("cymatrox.fluid.pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        let mk_bg = |src: &wgpu::Buffer, dst: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("cymatrox.fluid.bg"),
                layout: &bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: params_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: src.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: dst.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: out_buf.as_entire_binding(),
                    },
                ],
            })
        };
        let bind_groups = [
            mk_bg(&state_bufs[0], &state_bufs[1]),
            mk_bg(&state_bufs[1], &state_bufs[0]),
        ];

        Ok(Self {
            device,
            queue,
            pipeline,
            bind_groups,
            current: 0,
            out_buf,
            params_buf,
            staging_buf,
            out_count,
            config,
            time: 0.0,
        })
    }

    /// Live retune of the drive frequency — uniform rewrite only (I1).
    pub fn set_frequency(&mut self, frequency_hz: f32) {
        self.config.driving.frequency_hz = frequency_hz;
    }

    /// Live change of the vertical acceleration amplitude — I1.
    pub fn set_amplitude(&mut self, amplitude: f32) {
        self.config.driving.amplitude = amplitude;
    }

    /// Advances the simulation by one `dt` and returns the post-step
    /// surface nodes in strided row-major order (contract O1).
    ///
    /// Blocking by design ([ADR-0006](../../docs/adr/0006-gpu-cpu-readback-strategy.md)).
    pub fn step(&mut self) -> Result<Vec<FluidSurfaceNode>> {
        self.queue.write_buffer(
            &self.params_buf,
            0,
            bytemuck::bytes_of(&self.build_params()),
        );

        let byte_len = self.out_count as u64 * std::mem::size_of::<FluidSurfaceNode>() as u64;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("cymatrox.fluid.step"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("cymatrox.fluid.pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_groups[self.current], &[]);
            pass.dispatch_workgroups(
                self.config.surface.width.div_ceil(WORKGROUP),
                self.config.surface.height.div_ceil(WORKGROUP),
                1,
            );
        }
        encoder.copy_buffer_to_buffer(&self.out_buf, 0, &self.staging_buf, 0, byte_len);
        self.queue.submit(Some(encoder.finish()));

        let frame = Self::read_back(&self.device, &self.staging_buf, byte_len)?;

        // Ping-pong swap (invariant I3).
        self.current ^= 1;
        self.time += self.config.solver.dt;
        Ok(frame)
    }

    /// Non-blocking variant of [`step`](Self::step) for WASM and async contexts.
    #[cfg(feature = "web")]
    pub async fn step_async(&mut self) -> Result<Vec<FluidSurfaceNode>> {
        self.queue.write_buffer(
            &self.params_buf,
            0,
            bytemuck::bytes_of(&self.build_params()),
        );

        let byte_len = self.out_count as u64 * std::mem::size_of::<FluidSurfaceNode>() as u64;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("cymatrox.fluid.step_async"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("cymatrox.fluid.pass_async"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_groups[self.current], &[]);
            pass.dispatch_workgroups(
                self.config.surface.width.div_ceil(WORKGROUP),
                self.config.surface.height.div_ceil(WORKGROUP),
                1,
            );
        }
        encoder.copy_buffer_to_buffer(&self.out_buf, 0, &self.staging_buf, 0, byte_len);
        self.queue.submit(Some(encoder.finish()));

        let frame = crate::core::readback::read_back_async::<FluidSurfaceNode>(
            &self.staging_buf,
            byte_len,
            &self.device,
        )
        .await?;

        self.current ^= 1;
        self.time += self.config.solver.dt;
        Ok(frame)
    }

    fn build_params(&self) -> GpuParams {
        let g = &self.config.surface;
        let l = &self.config.liquid;
        GpuParams {
            grid_w: g.width,
            grid_h: g.height,
            out_w: self.config.output_dims().0,
            stride: g.readback_stride,
            dx: g.extent[0] / g.width as f32,
            dy: g.extent[1] / g.height as f32,
            gh_base: l.gravity * l.depth,
            sigma_h_rho: l.surface_tension * l.depth / l.density,
            damping_gamma: l.damping,
            dt: self.config.solver.dt,
            drive_omega: std::f32::consts::TAU * self.config.driving.frequency_hz,
            drive_accel_h: self.config.driving.amplitude * l.depth,
            time: self.time,
            radius_sq: match self.config.domain.shape {
                DomainShape::Circular { radius } => radius * radius,
                DomainShape::Full => -1.0,
            },
            centre_x: g.extent[0] * 0.5,
            centre_y: g.extent[1] * 0.5,
        }
    }

    /// ADR-0006 readback: map the staging buffer after a blocking poll,
    /// copy the bytes into a `Vec`, unmap.
    fn read_back(
        device: &wgpu::Device,
        staging: &wgpu::Buffer,
        byte_len: u64,
    ) -> Result<Vec<FluidSurfaceNode>> {
        let (tx, rx) = mpsc::channel();
        staging
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });

        device
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

        let out: Vec<FluidSurfaceNode> = {
            let view = staging.slice(..byte_len).get_mapped_range().map_err(|e| {
                Error::Gpu(GpuError::Readback(format!("mapped range unavailable: {e}")))
            })?;
            let bytes: &[u8] = &view;
            bytemuck::cast_slice(bytes).to_vec()
        };
        staging.unmap();

        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fluid::config::{Driving, LiquidSpec, SolverParams};

    fn reference_config(seed: u64) -> FluidConfig {
        FluidConfig {
            driving: Driving {
                frequency_hz: 60.0,
                amplitude: 90.0,
            },
            liquid: LiquidSpec {
                density: 1000.0,
                surface_tension: 0.072,
                depth: 0.004,
                damping: 0.8,
                gravity: 9.81,
            },
            surface: crate::fluid::config::SurfaceGrid {
                width: 96,
                height: 96,
                extent: [0.06, 0.06],
                readback_stride: 2,
                noise_amplitude: 1e-7,
                seed,
            },
            domain: crate::fluid::config::DomainMask {
                shape: DomainShape::Circular { radius: 0.025 },
            },
            solver: SolverParams { dt: 4e-5 },
        }
    }

    /// Contract O4/I3 — same seed ⇒ bit-identical trajectories across two
    /// independent simulations, ping-pong included.
    #[tokio::test]
    #[ignore = "requires a GPU-capable host (or software backend)"]
    async fn deterministic_same_seed() {
        let ctx = crate::GpuContext::new().await.expect("gpu context");
        let mut a = FluidSimulation::new(&ctx, reference_config(11)).expect("sim a");
        let mut b = FluidSimulation::new(&ctx, reference_config(11)).expect("sim b");
        for _ in 0..12 {
            assert_eq!(a.step().unwrap(), b.step().unwrap());
        }
    }

    /// Golden-file check vs the f64 reference oracle (ADR-0004/0007):
    /// mean |Δη| over returned nodes after 100 steps below frozen tolerance.
    #[tokio::test]
    #[ignore = "requires a GPU-capable host (or software backend)"]
    async fn golden_file_within_tolerance() {
        use crate::fluid::reference::ReferenceSim;

        let cfg = reference_config(123);
        let tolerance = GOLDEN_TOLERANCE_M;

        let ctx = crate::GpuContext::new().await.expect("gpu context");
        let mut gpu = FluidSimulation::new(&ctx, cfg).expect("gpu sim");
        let mut cpu = ReferenceSim::new(&reference_config(123));

        for i in 0..100 {
            let frame = gpu.step().unwrap();
            cpu.step();
            if i == 99 {
                let stride = reference_config(123).surface.readback_stride;
                let (out_w, out_h) = (48u32, 48u32);
                let expected: Vec<f64> = (0..out_h)
                    .flat_map(|oy| (0..out_w).map(move |ox| (ox, oy)))
                    .map(|(ox, oy)| cpu.height_at(ox * stride, oy * stride))
                    .collect();
                let mean_err: f64 = frame
                    .iter()
                    .zip(expected)
                    .map(|(node, ch)| (node.height as f64 - ch).abs())
                    .sum::<f64>()
                    / frame.len() as f64;
                assert!(
                    mean_err < tolerance,
                    "mean |Δη| {mean_err} exceeds frozen tolerance {tolerance}"
                );
            }
        }
    }

    /// Frozen after the first drift measurement landed on real hardware
    /// (docs/CONTRACT.md § Fluid, golden-file tolerance).
    const GOLDEN_TOLERANCE_M: f64 = 1.0e-11;

    /// step_async() must return bit-identical results to step().
    #[cfg(feature = "web")]
    #[tokio::test]
    #[ignore = "requires a GPU-capable host (or software backend)"]
    async fn step_async_matches_step() {
        let cfg = reference_config(42);
        let ctx = crate::GpuContext::new().await.expect("gpu context");

        let mut sim_sync = FluidSimulation::new(&ctx, cfg.clone()).expect("sync sim");
        let mut sim_async = FluidSimulation::new(&ctx, cfg).expect("async sim");

        let frame_sync = sim_sync.step().expect("step sync");
        let frame_async = sim_async.step_async().await.expect("step async");
        assert_eq!(frame_sync, frame_async);
    }
}
