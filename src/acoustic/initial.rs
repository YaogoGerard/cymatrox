//! Shared initial state — the single source both the GPU simulation and
//! the f64 reference oracle seed from, guaranteeing bit-identical starting
//! conditions (contract O4).

use crate::acoustic::config::AcousticConfig;
use crate::core::rng::Rng;

/// Zero pressure field plus seeded white noise (ADR-0012): the standing
/// wave needs a perturbation seed to break the perfectly-symmetric zero
/// solution. Row-major, x fastest.
pub(crate) fn initial_state(config: &AcousticConfig) -> Vec<(f32, f32)> {
    let g = &config.volume;
    let mut rng = Rng::new(g.seed);
    let n = (g.width * g.height * g.depth) as usize;
    let mut nodes = Vec::with_capacity(n);
    for _ in 0..n {
        let p = ((rng.next_f64() * 2.0 - 1.0) * g.noise_amplitude as f64) as f32;
        nodes.push((p, 0.0));
    }
    nodes
}
