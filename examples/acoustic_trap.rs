//! Acoustic radiation-force field (Gor'kov) in a standing-wave cavity —
//! minimal end-to-end usage of the acoustic module.
//!
//! Run with: `cargo run --example acoustic_trap`
//!
//! Requires a GPU-capable host (Vulkan / Metal / DX12 / WebGPU); there is
//! no CPU fallback by design ([ADR-0008](../docs/adr/0008-gpu-only-no-cpu-fallback.md)).

use cymatrox::acoustic::{
    AcousticConfig, AcousticSimulation, Axis, Driving, MediumSpec, ParticleSpec, Side,
    SolverParams, TransducerSpec, VolumeGrid,
};
use cymatrox::{GpuContext, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let ctx = GpuContext::new().await?;

    let config = AcousticConfig {
        driving: Driving {
            frequency_hz: 24_000.0,
            amplitude: 5.0,
        },
        medium: MediumSpec {
            density: 1.2041,
            sound_speed: 343.0,
        }, // air
        volume: VolumeGrid {
            width: 32,
            height: 32,
            depth: 32,
            extent: [0.04; 3],
            readback_stride: 2,
            noise_amplitude: 1e-9,
            seed: 123,
        },
        transducer: TransducerSpec {
            axis: Axis::X,
            side: Side::Low,
        },
        particle: ParticleSpec {
            radius: 1e-3, // 1 mm water droplet
            density: 1000.0,
            sound_speed: 1480.0,
        },
        solver: SolverParams {
            dt: 4e-7,
            averaging_periods: 8.0,
        },
    };

    let mut sim = AcousticSimulation::new(&ctx, config)?;
    println!("32³ air cavity driven at 24 kHz (X/Low face); EMA window ≈ 8 periods…");

    for step in 1..=100 {
        let frame = sim.step()?;
        if step % 25 == 0 {
            let max_p = frame
                .iter()
                .map(|n| n.pressure_pa.abs())
                .fold(0.0f32, f32::max);
            println!(
                "step {step:>3}: max |p| = {max_p:.1} Pa over {} nodes",
                frame.len()
            );
        }
    }

    // Gor'kov forces need a few averaging windows to converge.
    let frame = sim.step()?;
    let max_f = frame.iter().map(|n| norm(n.force)).fold(0.0f32, f32::max);
    println!("max |F| on a 1 mm water droplet: {max_f:.3e} N");
    Ok(())
}

fn norm(f: [f32; 3]) -> f32 {
    (f[0] * f[0] + f[1] * f[1] + f[2] * f[2]).sqrt()
}
