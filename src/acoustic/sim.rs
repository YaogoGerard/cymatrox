//! `AcousticSimulation` — the Phase 3 module entry point.
//!
//! Owns its GPU buffers/pipelines; receives the shared [`GpuContext`] by
//! reference at construction (ADR-0002). `step()` is deliberately blocking
//! and follows the staging-buffer readback pattern of ADR-0006.
//!
//! Two dispatches per step share one bind group (ADR-0012): `wave_step`
//! ping-pongs the `{p,q}` state (contract I3), then `gorkov` reads the
//! just-written buffer plus the EMA accumulators.

use crate::acoustic::config::{AcousticConfig, Axis, Side};
use crate::acoustic::initial::initial_state;
use crate::acoustic::types::{AcousticPressureNode, GpuParams};
use crate::{Error, GpuError, Result};
use std::sync::mpsc;
use wgpu::util::DeviceExt;

const WORKGROUP: u32 = 4;

/// Standing-wave levitation field in a rigid enclosure
/// (contract: docs/CONTRACT.md § Acoustic · model: ADR-0012).
pub struct AcousticSimulation {
    device: wgpu::Device,
    queue: wgpu::Queue,

    wave_pipeline: wgpu::ComputePipeline,
    gorkov_pipeline: wgpu::ComputePipeline,
    /// Ping-pong bind groups; both expose all five bindings.
    bind_groups: [wgpu::BindGroup; 2],
    current: usize,

    out_buf: wgpu::Buffer,
    params_buf: wgpu::Buffer,
    staging_buf: wgpu::Buffer,

    out_count: u32,
    config: AcousticConfig,
    /// Simulation time accumulated since construction (seconds).
    time: f32,
}

impl AcousticSimulation {
    /// Validates the configuration eagerly (contract F1) and builds every
    /// GPU resource once — later setters never reallocate (invariant I1).
    pub fn new(ctx: &crate::GpuContext, config: AcousticConfig) -> Result<Self> {
        config.validate()?;

        let device = ctx.device().clone();
        let queue = ctx.queue().clone();

        // ---- Initial state: zero pressure + seeded noise ----
        let initial = initial_state(&config);
        let state_bytes: Vec<u8> = initial
            .iter()
            .flat_map(|(p, q)| bytemuck::bytes_of(&[*p, *q]).to_vec())
            .collect();

        let state_bufs: [wgpu::Buffer; 2] = [
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("cymatrox.acoustic.state_a"),
                contents: &state_bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            }),
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("cymatrox.acoustic.state_b"),
                contents: &state_bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            }),
        ];

        let avg_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cymatrox.acoustic.averages"),
            size: state_bytes.len() as u64, // two f32 per node
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: true,
        });
        avg_buf.unmap(); // zero-initialized via mapped_at_creation

        let (ox, oy, _oz) = config.output_dims();
        let out_count = ox * oy * _oz;
        let out_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cymatrox.acoustic.out"),
            contents: &[0u8; std::mem::size_of::<AcousticPressureNode>()]
                .repeat(out_count as usize),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });
        let staging_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cymatrox.acoustic.staging"),
            size: out_count as u64 * std::mem::size_of::<AcousticPressureNode>() as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cymatrox.acoustic.params"),
            contents: &[0u8; std::mem::size_of::<GpuParams>()],
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // ---- Shaders & pipelines (one module, two entry points) ----
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cymatrox.acoustic.shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("acoustic.wgsl").into()),
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cymatrox.acoustic.bgl"),
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
                storage_entry(1, true),
                storage_entry(2, false),
                storage_entry(3, false),
                storage_entry(4, false),
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cymatrox.acoustic.pl"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });

        let mk_pipeline = |name: &'static str, entry: &'static str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(name),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let wave_pipeline = mk_pipeline("cymatrox.acoustic.wave", "wave_step");
        let gorkov_pipeline = mk_pipeline("cymatrox.acoustic.gorkov", "gorkov");

        let mk_bg = |src: &wgpu::Buffer, dst: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("cymatrox.acoustic.bg"),
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
                        resource: avg_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
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
            wave_pipeline,
            gorkov_pipeline,
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

    /// Live change of the transducer velocity amplitude — I1.
    pub fn set_amplitude(&mut self, amplitude: f32) {
        self.config.driving.amplitude = amplitude;
    }

    /// Advances the simulation by one `dt` and returns the post-step
    /// nodes in strided row-major order (contract O1).
    ///
    /// Blocking by design ([ADR-0006](../../docs/adr/0006-gpu-cpu-readback-strategy.md)).
    pub fn step(&mut self) -> Result<Vec<AcousticPressureNode>> {
        self.queue.write_buffer(
            &self.params_buf,
            0,
            bytemuck::bytes_of(&self.build_params()),
        );

        let byte_len = self.out_count as u64 * std::mem::size_of::<AcousticPressureNode>() as u64;
        let v = self.config.volume;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("cymatrox.acoustic.step"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("cymatrox.acoustic.pass"),
                timestamp_writes: None,
            });
            pass.set_bind_group(0, &self.bind_groups[self.current], &[]);
            pass.set_pipeline(&self.wave_pipeline);
            pass.dispatch_workgroups(
                v.width.div_ceil(WORKGROUP),
                v.height.div_ceil(WORKGROUP),
                v.depth.div_ceil(WORKGROUP),
            );
            // Ordered after wave_step within this pass; sees its writes.
            pass.set_pipeline(&self.gorkov_pipeline);
            pass.dispatch_workgroups(
                v.width.div_ceil(WORKGROUP),
                v.height.div_ceil(WORKGROUP),
                v.depth.div_ceil(WORKGROUP),
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
    pub async fn step_async(&mut self) -> Result<Vec<AcousticPressureNode>> {
        self.queue.write_buffer(
            &self.params_buf,
            0,
            bytemuck::bytes_of(&self.build_params()),
        );

        let byte_len = self.out_count as u64 * std::mem::size_of::<AcousticPressureNode>() as u64;
        let v = self.config.volume;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("cymatrox.acoustic.step_async"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("cymatrox.acoustic.pass_async"),
                timestamp_writes: None,
            });
            pass.set_bind_group(0, &self.bind_groups[self.current], &[]);
            pass.set_pipeline(&self.wave_pipeline);
            pass.dispatch_workgroups(
                v.width.div_ceil(WORKGROUP),
                v.height.div_ceil(WORKGROUP),
                v.depth.div_ceil(WORKGROUP),
            );
            pass.set_pipeline(&self.gorkov_pipeline);
            pass.dispatch_workgroups(
                v.width.div_ceil(WORKGROUP),
                v.height.div_ceil(WORKGROUP),
                v.depth.div_ceil(WORKGROUP),
            );
        }
        encoder.copy_buffer_to_buffer(&self.out_buf, 0, &self.staging_buf, 0, byte_len);
        self.queue.submit(Some(encoder.finish()));

        let frame = crate::core::readback::read_back_async::<AcousticPressureNode>(
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
        let v = &self.config.volume;
        let m = &self.config.medium;
        let par = &self.config.particle;
        let s = &self.solver_params_snapshot();
        let f = self.config.driving.frequency_hz;
        let rho0 = m.density;
        let c = m.sound_speed;

        // Gor'kov coefficients (ADR-0012).
        let v0 = 4.0 / 3.0 * std::f32::consts::PI * par.radius.powi(3);
        let f1 = 1.0 - rho0 * c * c / ((par.density * par.sound_speed).powi(2));
        let f2 = 2.0 * (par.density - m.density) / (2.0 * par.density + m.density);
        let omega = std::f32::consts::TAU * f;
        let tau = s.averaging_periods / f;

        let (axis, side) = match (&self.config.transducer.axis, &self.config.transducer.side) {
            (Axis::X, Side::Low) => (0, 0),
            (Axis::X, Side::High) => (0, 1),
            (Axis::Y, Side::Low) => (1, 0),
            (Axis::Y, Side::High) => (1, 1),
            (Axis::Z, Side::Low) => (2, 0),
            (Axis::Z, Side::High) => (2, 1),
        };

        GpuParams {
            grid_x: v.width,
            grid_y: v.height,
            grid_z: v.depth,
            out_x: self.output_dims().0,
            out_y: self.output_dims().1,
            stride: v.readback_stride,
            axis,
            side,
            dx: v.extent[0] / v.width as f32,
            dy: v.extent[1] / v.height as f32,
            dz: v.extent[2] / v.depth as f32,
            c2: c * c,
            rho0,
            dt: s.dt,
            omega,
            drive_u: self.config.driving.amplitude,
            time: self.time,
            ema_alpha: 1.0 - (-(s.dt / tau)).exp(),
            neumann_amp: rho0 * omega * self.config.driving.amplitude,
            _pad0: 0.0,
            gk_p_coeff: v0 * f1 / (2.0 * rho0 * c * c),
            gk_g_coeff: -v0 * 3.0 * f2 / (4.0 * rho0 * rho0 * omega * omega),
            _pad1: 0.0,
            _pad2: 0.0,
        }
    }

    fn solver_params_snapshot(&self) -> crate::acoustic::config::SolverParams {
        self.config.solver
    }

    fn output_dims(&self) -> (u32, u32, u32) {
        self.config.output_dims()
    }

    /// ADR-0006 readback: map the staging buffer after a blocking poll,
    /// copy the bytes into a `Vec`, unmap.
    fn read_back(
        device: &wgpu::Device,
        staging: &wgpu::Buffer,
        byte_len: u64,
    ) -> Result<Vec<AcousticPressureNode>> {
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

        let out: Vec<AcousticPressureNode> = {
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

fn storage_entry(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acoustic::config::{
        Driving, MediumSpec, ParticleSpec, SolverParams, TransducerSpec,
    };

    fn reference_config(seed: u64) -> AcousticConfig {
        AcousticConfig {
            // Detuned off any box eigenfrequency (c/2L = 4287.5 Hz here;
            // 25 kHz would sit exactly on mode (3,3,4), where the undamped
            // linear growth amplifies f32 round-off beyond useful bounds).
            driving: Driving {
                frequency_hz: 24_000.0,
                amplitude: 5.0,
            },
            medium: MediumSpec {
                density: 1.2041,
                sound_speed: 343.0,
            },
            volume: crate::acoustic::config::VolumeGrid {
                width: 48,
                height: 48,
                depth: 48,
                extent: [0.04, 0.04, 0.04],
                readback_stride: 2,
                noise_amplitude: 1e-9,
                seed,
            },
            transducer: TransducerSpec {
                axis: Axis::X,
                side: Side::Low,
            },
            particle: ParticleSpec {
                radius: 1e-3,
                density: 1000.0,
                sound_speed: 1480.0,
            },
            solver: SolverParams {
                dt: 4e-7,
                averaging_periods: 8.0,
            },
        }
    }

    /// Contract O4/I3 — same seed ⇒ bit-identical trajectories across two
    /// independent simulations, ping-pong included.
    #[tokio::test]
    #[ignore = "requires a GPU-capable host (or software backend)"]
    async fn deterministic_same_seed() {
        let ctx = crate::GpuContext::new().await.expect("gpu context");
        let mut a = AcousticSimulation::new(&ctx, reference_config(21)).expect("sim a");
        let mut b = AcousticSimulation::new(&ctx, reference_config(21)).expect("sim b");
        for _ in 0..10 {
            assert_eq!(a.step().unwrap(), b.step().unwrap());
        }
    }

    /// Golden-file check vs the f64 reference oracle (ADR-0004/0007):
    /// mean |Δp| over returned nodes after 100 steps below frozen tolerance.
    #[tokio::test]
    #[ignore = "requires a GPU-capable host (or software backend)"]
    async fn golden_file_within_tolerance() {
        use crate::acoustic::reference::ReferenceSim;

        let cfg = reference_config(123);
        let tolerance = GOLDEN_TOLERANCE_PA;

        let ctx = crate::GpuContext::new().await.expect("gpu context");
        let mut gpu = AcousticSimulation::new(&ctx, cfg).expect("gpu sim");
        let mut cpu = ReferenceSim::new(&reference_config(123));

        for i in 0..100 {
            let frame = gpu.step().unwrap();
            cpu.step();
            if i == 99 {
                let stride = reference_config(123).volume.readback_stride;
                let (ox, oy, oz) = (24u32, 24u32, 24u32);
                let expected: Vec<(f64, [f64; 3])> = (0..oz)
                    .flat_map(|gz| (0..oy).flat_map(move |gy| (0..ox).map(move |gx| (gx, gy, gz))))
                    .map(|(gx, gy, gz)| cpu.node_at(gx * stride, gy * stride, gz * stride))
                    .collect();
                assert_eq!(frame.len(), expected.len());
                let mean_err: f64 = frame
                    .iter()
                    .zip(expected)
                    .map(|(n, (cp, _))| (n.pressure_pa as f64 - cp).abs())
                    .sum::<f64>()
                    / frame.len() as f64;
                assert!(
                    mean_err < tolerance,
                    "mean |Δp| {mean_err} exceeds frozen tolerance {tolerance}"
                );
            }
        }
    }

    /// Frozen after the first drift measurement landed on real hardware
    /// (docs/CONTRACT.md § Acoustic, golden-file tolerance).
    /// Frozen after the first drift measurement landed on real hardware:
    /// mean |Δp| = 1.585e-2 Pa over the 100-step golden horizon (field
    /// scale O(10³) Pa ⇒ ~1e-5 relative, consistent with f32
    /// accumulation). Frozen two orders above with margin
    /// (docs/CONTRACT.md § Acoustic).
    const GOLDEN_TOLERANCE_PA: f64 = 0.25;

    /// step_async() must return bit-identical results to step().
    #[cfg(feature = "web")]
    #[tokio::test]
    #[ignore = "requires a GPU-capable host (or software backend)"]
    async fn step_async_matches_step() {
        let cfg = reference_config(42);
        let ctx = crate::GpuContext::new().await.expect("gpu context");

        let mut sim_sync = AcousticSimulation::new(&ctx, cfg).expect("sync sim");
        let mut sim_async = AcousticSimulation::new(&ctx, cfg).expect("async sim");

        let frame_sync = sim_sync.step().expect("step sync");
        let frame_async = sim_async.step_async().await.expect("step async");
        assert_eq!(frame_sync, frame_async);
    }
}
