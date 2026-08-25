//! Integration tests exercising the acoustic module through its public API
//! on real GPU-capable hosts (ADR-0007: run in CI under Lavapipe).

use cymatrox::acoustic::{
    AcousticConfig, Axis, Driving, MAX_GRID_DIM, MediumSpec, ParticleSpec, Side, SolverParams,
    TransducerSpec, VolumeGrid,
};
use cymatrox::{GpuContext, Result};

fn config(seed: u64, stride: u32) -> AcousticConfig {
    AcousticConfig {
        driving: Driving {
            frequency_hz: 25_000.0,
            amplitude: 5.0,
        },
        medium: MediumSpec {
            density: 1.2041,
            sound_speed: 343.0,
        },
        volume: VolumeGrid {
            width: 40,
            height: 40,
            depth: 40,
            extent: [0.04, 0.04, 0.04],
            readback_stride: stride,
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

/// Contract O1/O2 — stable strided length and finite values across many
/// steps, including live retuning mid-run.
#[tokio::test]
#[ignore = "requires a GPU-capable host (or software backend)"]
async fn steps_preserve_shape_and_finiteness() -> Result<()> {
    let ctx = GpuContext::new().await?;
    let mut sim = cymatrox::acoustic::AcousticSimulation::new(&ctx, config(1, 2))?;
    let expected_len = 20 * 20 * 20;

    for i in 0..60 {
        if i == 30 {
            sim.set_frequency(30_000.0);
            sim.set_amplitude(8.0);
        }
        let frame = sim.step()?;
        assert_eq!(frame.len(), expected_len, "O1 violated at step {i}");
        for node in &frame {
            assert!(
                node.pressure_pa.is_finite() && node.force.iter().all(|f| f.is_finite()),
                "O2 violated at step {i}"
            );
        }
    }
    Ok(())
}

/// Physical sanity — a full-face transducer on X/Low with a cubic isotropic
/// grid produces a field symmetric under Y and Z reflections (noise-free
/// initial state).
#[tokio::test]
#[ignore = "requires a GPU-capable host (or software backend)"]
async fn field_is_symmetric_across_midplanes() -> Result<()> {
    let ctx = GpuContext::new().await?;
    let mut cfg = config(7, 1);
    cfg.volume.noise_amplitude = 0.0; // keep the setup perfectly symmetric
    let mut sim = cymatrox::acoustic::AcousticSimulation::new(&ctx, cfg)?;

    let n = 40u32;
    for _ in 0..80 {
        let frame = sim.step()?;
        let at = |x: usize, y: usize, z: usize| -> &[f32; 3] {
            &frame[(z * n as usize + y) * n as usize + x].force
        };
        // Compare a slice of mirrored pairs across both mid-planes.
        for k in [5usize, 12, 20, 27] {
            for j in [5usize, 12, 33] {
                for i in [10usize, 25] {
                    let a = at(i, j, k);
                    let b = at(i, n as usize - 1 - j, k);
                    for d in 0..3 {
                        assert!(
                            (a[d] - b[d]).abs() < 1e-9,
                            "Y-symmetry broken at ({i},{j},{k}): {a:?} vs {b:?}"
                        );
                    }
                    let c = at(i, j, n as usize - 1 - k);
                    for d in 0..3 {
                        assert!(
                            (a[d] - c[d]).abs() < 1e-9,
                            "Z-symmetry broken at ({i},{j},{k})"
                        );
                    }
                }
            }
        }
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

        let mut cfg = config(1, 1);
        cfg.volume.depth = MAX_GRID_DIM + 4;
        let err = cymatrox::acoustic::AcousticSimulation::new(&ctx, cfg)
            .err()
            .expect("oversized grid must fail");
        assert!(err.to_string().contains("P1"), "{err}");

        let mut cfg = config(1, 1);
        cfg.particle.radius = 1.0; // λ/4 ≈ 3.4 mm at 25 kHz
        let err = cymatrox::acoustic::AcousticSimulation::new(&ctx, cfg)
            .err()
            .expect("Gor'kov validity violation must fail");
        assert!(err.to_string().contains("P2"), "{err}");

        let mut cfg = config(1, 1);
        cfg.driving.frequency_hz = 15.0;
        let err = cymatrox::acoustic::AcousticSimulation::new(&ctx, cfg)
            .err()
            .expect("out-of-range frequency must fail");
        assert!(err.to_string().contains("P3"), "{err}");

        let mut cfg = config(1, 1);
        cfg.solver.dt = 1e-3; // wildly above the CFL bound
        let err = cymatrox::acoustic::AcousticSimulation::new(&ctx, cfg)
            .err()
            .expect("CFL violation must fail");
        assert!(err.to_string().contains("P4"), "{err}");
    });
}
