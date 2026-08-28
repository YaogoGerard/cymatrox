//! CPU reference implementation of the granular solver in `f64` — the
//! correctness oracle of ADR-0004.
//!
//! Compiled only under the `reference` feature or during tests; never part
//! of the release build's runtime path. Mirrors `granular.wgsl` exactly,
//! including wall-bounce edge cases. Layout/physics changes must be applied
//! to both files in the same commit.
//!
//! This oracle is intentionally not part of the runtime path; it exists as a
//! validation aid and may legitimately contain helper methods that are only
//! exercised in golden-file tests.

#![allow(dead_code)]

use super::config::{GranularConfig, ResolvedMode};
use super::placement::place_grains;
use crate::core::rng::Rng;

#[derive(Clone, Copy)]
struct Grain {
    x: f64,
    y: f64,
    vx: f64,
    vy: f64,
}

pub(crate) struct ReferenceSim {
    side: f64,
    dt: f64,
    drag: f64,
    restitution: f64,
    coupling_k: f64,
    grains: Vec<Grain>,
    time: f64,
}

const TAU: f64 = std::f64::consts::TAU;

impl ReferenceSim {
    pub(crate) fn new(config: &GranularConfig) -> Self {
        let side = match config.medium {
            super::config::PlateSpec::Idealized { side } => side as f64,
        };
        let s = &config.solver;
        let mut rng = Rng::new(config.grains.seed);
        let grains = place_grains(config, &mut rng, side)
            .into_iter()
            .map(|(x, y)| Grain {
                x,
                y,
                vx: 0.0,
                vy: 0.0,
            })
            .collect();

        Self {
            side,
            dt: s.dt as f64,
            drag: s.drag as f64,
            restitution: s.restitution as f64,
            coupling_k: s.coupling_k as f64,
            grains,
            time: 0.0,
        }
    }

    /// One semi-implicit Euler step; mirrors the WGSL shader line for line.
    pub(crate) fn step(&mut self, modes: &[ResolvedMode], amplitude: f32) {
        let amp = amplitude as f64;
        let l = self.side;
        let pi_over_l = std::f64::consts::PI / l;

        // Snapshot forces so all updates use pre-step state.
        let forces: Vec<(f64, f64)> = self
            .grains
            .iter()
            .map(|g| {
                let mut w = 0.0;
                let mut gx = 0.0;
                let mut gy = 0.0;
                for m in modes {
                    let am = m.m as f64 * pi_over_l;
                    let an = m.n as f64 * pi_over_l;
                    let sx = (am * g.x).sin();
                    let cx = (am * g.x).cos();
                    let sy = (an * g.y).sin();
                    let cy = (an * g.y).cos();
                    let phase = (TAU * m.omega_hz as f64 * self.time).cos();
                    w += amp * sx * sy * phase;
                    gx += amp * phase * am * cx * sy;
                    gy += amp * phase * sx * an * cy;
                }
                (
                    -self.coupling_k * 2.0 * w * gx,
                    -self.coupling_k * 2.0 * w * gy,
                )
            })
            .collect();

        let damping = (-self.drag * self.dt).exp();
        let two_l = 2.0 * self.side;
        for (g, (fx, fy)) in self.grains.iter_mut().zip(forces) {
            g.vx = (g.vx + fx * self.dt) * damping;
            g.vy = (g.vy + fy * self.dt) * damping;
            g.x += g.vx * self.dt;
            g.y += g.vy * self.dt;

            if g.x < 0.0 {
                g.x = -g.x;
                g.vx = -g.vx * self.restitution;
            } else if g.x > l {
                g.x = two_l - g.x;
                g.vx = -g.vx * self.restitution;
            }
            if g.y < 0.0 {
                g.y = -g.y;
                g.vy = -g.vy * self.restitution;
            } else if g.y > l {
                g.y = two_l - g.y;
                g.vy = -g.vy * self.restitution;
            }
        }

        self.time += self.dt;
    }

    pub(crate) fn positions(&self) -> impl Iterator<Item = (f64, f64)> + '_ {
        self.grains.iter().map(|g| (g.x, g.y))
    }
}
