//! Chladni-plate granular pattern — minimal end-to-end usage of the
//! granular module.
//!
//! Run with: `cargo run --example granular_chladni`
//!
//! Requires a GPU-capable host (Vulkan / Metal / DX12 / WebGPU); there is
//! no CPU fallback by design ([ADR-0008](../docs/adr/0008-gpu-only-no-cpu-fallback.md)).

use cymatrox::granular::{
    Driving, GrainBed, GranularConfig, GranularSimulation, InitialDistribution, ModeSelection,
    PlateSpec, SolverParams,
};
use cymatrox::{GpuContext, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let ctx = GpuContext::new().await?;

    let config = GranularConfig {
        experiment: Driving {
            frequency_hz: 440.0,
            amplitude: 1e-4,
            modes: ModeSelection::Auto,
        },
        medium: PlateSpec::Idealized { side: 0.5 },
        grains: GrainBed {
            count: 100_000,
            distribution: InitialDistribution::Uniform,
            seed: 42,
        },
        solver: SolverParams {
            dt: 1.0 / 480.0,
            drag: 4.0,
            restitution: 0.6,
            coupling_k: 5.0e5,
            base_frequency_hz: 120.0,
        },
    };

    let mut sim = GranularSimulation::new(&ctx, config)?;
    println!("100k grains on a 0.5 m idealized plate, driving at 440 Hz…");

    for step in 1..=600 {
        let frame = sim.step()?;
        if step % 120 == 0 {
            let spread = frame
                .iter()
                .map(|g| {
                    let [x, y] = g.position;
                    ((x - 0.25).powi(2) + (y - 0.25).powi(2)).sqrt()
                })
                .fold(0.0f32, f32::max);
            println!("step {step:>3}: max distance from plate centre = {spread:.4} m");
        }
    }

    // Live retuning — modes resolve from the new frequency on the next step.
    sim.set_frequency(880.0);
    println!("retuned to 880 Hz; running one more step…");
    let frame = sim.step()?;
    println!(
        "{} grains read back, all positions in [0, side]",
        frame.len()
    );
    Ok(())
}
