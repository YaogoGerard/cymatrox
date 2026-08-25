//! Integration tests exercising the granular module through its public API
//! on real GPU-capable hosts (ADR-0007: run in CI under Lavapipe).

use cymatrox::granular::{
    Driving, GrainBed, GranularConfig, InitialDistribution, ModeSelection, PlateSpec, SolverParams,
};
use cymatrox::{GpuContext, Result};

fn config(seed: u64, count: u32) -> GranularConfig {
    GranularConfig {
        experiment: Driving {
            frequency_hz: 440.0,
            amplitude: 1e-4,
            modes: ModeSelection::Auto,
        },
        medium: PlateSpec::Idealized { side: 0.5 },
        grains: GrainBed {
            count,
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

/// Contract O1/O2 — stable length and confined positions across many steps,
/// including live retuning mid-run.
#[tokio::test]
#[ignore = "requires a GPU-capable host (or software backend)"]
async fn steps_preserve_shape_and_bounds() -> Result<()> {
    let ctx = GpuContext::new().await?;
    let side = 0.5;
    let mut sim = cymatrox::granular::GranularSimulation::new(&ctx, config(1, 8192))?;

    for i in 0..60 {
        if i == 30 {
            sim.set_frequency(950.0);
            sim.set_amplitude(2e-4);
        }
        let frame = sim.step()?;
        assert_eq!(frame.len(), 8192, "O1 violated at step {i}");
        for g in &frame {
            assert!(
                g.position[0].is_finite()
                    && g.position[1].is_finite()
                    && (0.0..=side).contains(&g.position[0])
                    && (0.0..=side).contains(&g.position[1]),
                "O2 violated at step {i}: {:?}",
                g.position
            );
        }
    }
    Ok(())
}

/// Contract F1 — invalid configurations must be rejected eagerly with
/// actionable messages naming the clause.
#[test]
fn invalid_configs_are_rejected() {
    // No GPU needed: validation happens before any device work.
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    rt.block_on(async {
        let ctx = GpuContext::new().await.expect("gpu context");

        let mut cfg = config(1, 100);
        cfg.grains.count = 0;
        let err = cymatrox::granular::GranularSimulation::new(&ctx, cfg)
            .err()
            .expect("count=0 must fail");
        assert!(err.to_string().contains("P1"), "{err}");

        let mut cfg = config(1, 100);
        cfg.experiment.frequency_hz = 30_000.0;
        let err = cymatrox::granular::GranularSimulation::new(&ctx, cfg)
            .err()
            .expect("frequency out of range must fail");
        assert!(err.to_string().contains("P3"), "{err}");

        let mut cfg = config(1, 100);
        cfg.solver.restitution = 1.5;
        let err = cymatrox::granular::GranularSimulation::new(&ctx, cfg)
            .err()
            .expect("restitution > 1 must fail");
        assert!(err.to_string().contains("P2"), "{err}");

        let mut cfg = config(1, 100);
        cfg.experiment.modes = ModeSelection::Explicit(vec![(0, 3)]);
        let err = cymatrox::granular::GranularSimulation::new(&ctx, cfg)
            .err()
            .expect("mode index 0 must fail");
        assert!(err.to_string().contains("P4"), "{err}");
    });
}

/// Measured-mode selection semantics (ADR-0009): the eigenpair closest to
/// the requested frequency wins, recomputed live after retune.
#[tokio::test]
#[ignore = "requires a GPU-capable host (or software backend)"]
async fn measured_mode_source_runs() -> Result<()> {
    let ctx = GpuContext::new().await?;
    let mut cfg = config(3, 1024);
    cfg.experiment.modes = ModeSelection::Measured(vec![
        cymatrox::granular::EigenPair {
            m: 2,
            n: 1,
            omega_measured_hz: 430.0,
        },
        cymatrox::granular::EigenPair {
            m: 1,
            n: 3,
            omega_measured_hz: 610.0,
        },
    ]);
    let mut sim = cymatrox::granular::GranularSimulation::new(&ctx, cfg)?;
    sim.step()?; // selects (2,1) @ 430 Hz
    sim.set_frequency(600.0);
    sim.step()?; // re-selects (1,3) @ 610 Hz — no reallocation happened (I1)
    Ok(())
}
