//! GPU-accelerated scientific toolkit for cymatics simulation.
//!
//! Three independent modules share one [`GpuContext`]:
//!
//! * [`granular`] — solids on Chladni plates (modal superposition)
//! * [`fluid`] — liquid surfaces under vertical vibration (Faraday waves)
//! * [`acoustic`] — standing-wave pressure fields and Gor'kov radiation forces
//!
//! Only context creation is async; every [`step()`][granular::GranularSimulation::step]
//! is a deliberate blocking call returning the post-step state
//! ([ADR-0006](https://github.com/YaogoGerard/cymatrox/blob/main/docs/adr/0006-gpu-cpu-readback-strategy.md)).
//!
//! A GPU-capable host is required — there is no CPU fallback by design
//! ([ADR-0008](https://github.com/YaogoGerard/cymatrox/blob/main/docs/adr/0008-gpu-only-no-cpu-fallback.md)).
//!
//! # Example
//!
//! ```no_run
//! use cymatrox::{GpuContext, acoustic::{
//!     AcousticConfig, AcousticSimulation, Axis, Driving, MediumSpec,
//!     ParticleSpec, Side, SolverParams, TransducerSpec, VolumeGrid,
//! }};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), cymatrox::Error> {
//!     let ctx = GpuContext::new().await?;
//!
//!     let config = AcousticConfig {
//!         driving: Driving { frequency_hz: 24_000.0, amplitude: 5.0 },
//!         medium: MediumSpec { density: 1.2041, sound_speed: 343.0 },
//!         volume: VolumeGrid {
//!             width: 32, height: 32, depth: 32,
//!             extent: [0.04; 3],
//!             readback_stride: 2,
//!             noise_amplitude: 1e-9,
//!             seed: 123,
//!         },
//!         transducer: TransducerSpec { axis: Axis::X, side: Side::Low },
//!         particle: ParticleSpec {
//!             radius: 1e-3, density: 1000.0, sound_speed: 1480.0,
//!         },
//!         solver: SolverParams { dt: 4e-7, averaging_periods: 8.0 },
//!     };
//!
//!     let mut sim = AcousticSimulation::new(&ctx, config)?;
//!     let frame = sim.step()?;
//!     println!("{} nodes read back", frame.len());
//!     Ok(())
//! }
//! ```
//!
//! See `docs/ARCHITECTURE.md` and `docs/CONTRACT.md`.

pub mod acoustic;
pub mod core;
pub mod fluid;
pub mod granular;

pub use core::context::GpuContext;
pub use core::error::{Error, GpuError, Result};
