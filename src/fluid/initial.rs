//! Shared initial state — the single source both the GPU simulation and
//! the f64 reference oracle seed from, guaranteeing bit-identical starting
//! conditions (contract O4).

use crate::core::rng::Rng;
use crate::fluid::config::FluidConfig;
use crate::fluid::types::FluidSurfaceNode;

/// Flat surface plus seeded white noise (ADR-0011): the Faraday
/// instability needs a perturbation to grow from. Row-major order.
pub(crate) fn initial_state(config: &FluidConfig) -> Vec<FluidSurfaceNode> {
    let g = &config.surface;
    let mut rng = Rng::new(g.seed);
    let mut nodes = Vec::with_capacity((g.width * g.height) as usize);
    for _ in 0..g.height {
        for _ in 0..g.width {
            let eta = ((rng.next_f64() * 2.0 - 1.0) * g.noise_amplitude as f64) as f32;
            nodes.push(FluidSurfaceNode {
                height: eta,
                velocity_y: 0.0,
            });
        }
    }
    nodes
}
