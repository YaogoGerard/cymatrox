//! Integration tests exercising the fluid module through its public API
//! on real GPU-capable hosts (ADR-0007: run in CI under Lavapipe).

use cymatrox::fluid::{
    DomainMask, DomainShape, Driving, FluidConfig, LiquidSpec, MAX_GRID_DIM, SolverParams,
    SurfaceGrid,
};
use cymatrox::{GpuContext, Result};

fn config(seed: u64, stride: u32, shape: DomainShape) -> FluidConfig {
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
        surface: SurfaceGrid {
            width: 96,
            height: 64,
            extent: [0.06, 0.04],
            readback_stride: stride,
            noise_amplitude: 1e-7,
            seed,
        },
        domain: DomainMask { shape },
        solver: SolverParams { dt: 4e-5 },
    }
}

/// Contract O1/O2 — stable strided length and finite heights across many
/// steps, including live retuning mid-run; masked-out nodes pinned at zero.
#[tokio::test]
#[ignore = "requires a GPU-capable host (or software backend)"]
async fn steps_preserve_shape_bounds_and_mask() -> Result<()> {
    let ctx = GpuContext::new().await?;
    let (w, h, s) = (96u32, 64u32, 2u32);
    let radius = 0.015_f32;
    let mut sim = cymatrox::fluid::FluidSimulation::new(
        &ctx,
        config(1, s, DomainShape::Circular { radius }),
    )?;

    let expected_len = (w.div_ceil(s) * h.div_ceil(s)) as usize;
    let cx = 0.06 / 2.0;
    let cy = 0.04 / 2.0;
    let dx = 0.06 / w as f32;

    for i in 0..60 {
        if i == 30 {
            sim.set_frequency(120.0);
            sim.set_amplitude(150.0);
        }
        let frame = sim.step()?;
        assert_eq!(frame.len(), expected_len, "O1 violated at step {i}");
        for (k, node) in frame.iter().enumerate() {
            assert!(
                node.height.is_finite() && node.velocity_y.is_finite(),
                "O2 violated at step {i}: non-finite value"
            );
            // Rebuild the grid coordinates of this strided node.
            let ox = (k as u32 % w.div_ceil(s)) * s;
            let oy = (k as u32 / w.div_ceil(s)) * s;
            let px = ox as f32 * dx;
            let py = oy as f32 * dx; // square cells in this config
            // Margin of one cell² absorbs f32 noise for nodes sitting
            // exactly on the rim; anything clearly beyond must be pinned.
            let outside = (px - cx).powi(2) + (py - cy).powi(2) > radius * radius + dx * dx;
            if outside {
                assert_eq!(
                    (node.height, node.velocity_y),
                    (0.0, 0.0),
                    "O2 violated at step {i}: rim node ({ox},{oy}) not pinned"
                );
            }
        }
    }
    Ok(())
}

/// `Full` domain — no mask, whole rectangular buffer active.
#[tokio::test]
#[ignore = "requires a GPU-capable host (or software backend)"]
async fn full_domain_runs() -> Result<()> {
    let ctx = GpuContext::new().await?;
    let mut sim = cymatrox::fluid::FluidSimulation::new(&ctx, config(5, 1, DomainShape::Full))?;
    for _ in 0..20 {
        let frame = sim.step()?;
        assert_eq!(frame.len(), (96 * 64) as usize);
    }
    Ok(())
}

/// Contract F1 — invalid configurations must be rejected eagerly with
/// actionable messages naming the clause.
#[test]
fn invalid_configs_are_rejected() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    rt.block_on(async {
        let ctx = GpuContext::new().await.expect("gpu context");

        let mut cfg = config(1, 1, DomainShape::Full);
        cfg.surface.width = MAX_GRID_DIM + 8;
        let err = cymatrox::fluid::FluidSimulation::new(&ctx, cfg)
            .err()
            .expect("oversized grid must fail");
        assert!(err.to_string().contains("P1"), "{err}");

        let cfg = config(1, 1, DomainShape::Circular { radius: 10.0 });
        let err = cymatrox::fluid::FluidSimulation::new(&ctx, cfg)
            .err()
            .expect("dish larger than extent must fail");
        assert!(err.to_string().contains("P1"), "{err}");

        let mut cfg = config(1, 1, DomainShape::Full);
        cfg.liquid.density = -1.0;
        let err = cymatrox::fluid::FluidSimulation::new(&ctx, cfg)
            .err()
            .expect("negative density must fail");
        assert!(err.to_string().contains("P2"), "{err}");

        let mut cfg = config(1, 1, DomainShape::Full);
        cfg.driving.frequency_hz = 50_000.0;
        let err = cymatrox::fluid::FluidSimulation::new(&ctx, cfg)
            .err()
            .expect("out-of-range frequency must fail");
        assert!(err.to_string().contains("P3"), "{err}");

        let mut cfg = config(1, 1, DomainShape::Full);
        cfg.solver.dt = 1.0; // wildly above the CFL/stability bound
        let err = cymatrox::fluid::FluidSimulation::new(&ctx, cfg)
            .err()
            .expect("unstable dt must fail");
        assert!(err.to_string().contains("P4"), "{err}");
    });
}
