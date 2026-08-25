//! Initial grain placement shared by the GPU path and the f64 reference,
//! so both start from identical state (contract O4/I2).
//!
//! SplitMix64-driven, dependency-free, platform-stable.

use super::config::{GranularConfig, InitialDistribution};
use crate::core::rng::Rng;

/// Returns the initial `(x, y)` position of every grain in `[0, side]²`.
pub(crate) fn place_grains(config: &GranularConfig, rng: &mut Rng, side: f64) -> Vec<(f64, f64)> {
    let n = config.grains.count as usize;
    match config.grains.distribution {
        InitialDistribution::Uniform => (0..n)
            .map(|_| (rng.next_f64() * side, rng.next_f64() * side))
            .collect(),
        InitialDistribution::CenteredCluster => {
            let r = side * 0.05;
            let c = side * 0.5;
            (0..n)
                .map(|_| {
                    (
                        c + (rng.next_f64() - 0.5) * 2.0 * r,
                        c + (rng.next_f64() - 0.5) * 2.0 * r,
                    )
                })
                .collect()
        }
        InitialDistribution::Grid => {
            let cells = (n as f64).sqrt().ceil().max(1.0);
            let cell = side / cells;
            (0..n)
                .map(|i| {
                    let gx = i % cells as usize;
                    let gy = i / cells as usize;
                    (
                        ((gx as f64 + 0.5 + (rng.next_f64() - 0.5) * 0.5) * cell).min(side),
                        ((gy as f64 + 0.5 + (rng.next_f64() - 0.5) * 0.5) * cell).min(side),
                    )
                })
                .collect()
        }
    }
}
