//! Faraday ripples on a vibrating liquid surface — minimal end-to-end
//! usage of the fluid module.
//!
//! Run with: `cargo run --example fluid_ripples`
//!
//! Requires a GPU-capable host (Vulkan / Metal / DX12 / WebGPU); there is
//! no CPU fallback by design ([ADR-0008](../docs/adr/0008-gpu-only-no-cpu-fallback.md)).

use cymatrox::fluid::{
    DomainMask, DomainShape, Driving, FluidConfig, FluidSimulation, LiquidSpec, SolverParams,
    SurfaceGrid,
};
use cymatrox::{GpuContext, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let ctx = GpuContext::new().await?;

    let config = FluidConfig {
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
        surface: SurfaceGrid {
            width: 96,
            height: 96,
            extent: [0.06, 0.06],
            readback_stride: 2,
            noise_amplitude: 1e-7,
            seed: 7,
        },
        domain: DomainMask {
            shape: DomainShape::Circular { radius: 0.025 },
        },
        solver: SolverParams { dt: 4e-5 },
    };

    let mut sim = FluidSimulation::new(&ctx, config)?;
    println!("water dish (r = 2.5 cm) driven at 60 Hz…");

    let mut peak = f32::MIN;
    for step in 1..=500 {
        let frame = sim.step()?;
        if step % 100 == 0 {
            // Strided readback is row-major, x-fastest (contract O1).
            let max = frame.iter().map(|n| n.height.abs()).fold(0.0f32, f32::max);
            peak = peak.max(max);
            println!(
                "step {step:>3}: max |η| = {max:.3e} m over {} nodes",
                frame.len()
            );
        }
    }

    // Live retuning mid-run — no reallocation, next step uses the new drive.
    sim.set_frequency(120.0);
    sim.set_amplitude(120.0);
    println!("retuned to 120 Hz / a = 120 m/s²; running one more step…");
    println!("peak elevation so far: {peak:.3e} m");
    Ok(())
}
