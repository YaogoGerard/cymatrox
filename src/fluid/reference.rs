//! Double-precision CPU oracle mirroring `fluid.wgsl` line for line
//! ([ADR-0004](../../../docs/adr/0004-numerical-precision-strategy.md),
//! [ADR-0007](../../../docs/adr/0007-test-strategy.md)).
//!
//! Test-only (`cfg(any(test, feature = "reference"))`). The operation
//! order deliberately matches the shader — same stencil traversal, same
//! update sequence — so drift stays within the contract tolerance instead
//! of compounding through reordering.

use crate::fluid::config::{DomainShape, FluidConfig};

#[derive(Clone, Copy)]
struct Node {
    height: f64,
    velocity_y: f64,
}

pub(crate) struct ReferenceSim {
    w: u32,
    h: u32,
    dx: f64,
    dy: f64,
    gh_base: f64,
    sigma_h_rho: f64,
    gamma: f64,
    dt: f64,
    omega: f64,
    accel_h: f64,
    radius_sq: f64,
    centre_x: f64,
    centre_y: f64,
    cur: Vec<Node>,
    next: Vec<Node>,
    time: f64,
}

impl ReferenceSim {
    pub(crate) fn new(config: &FluidConfig) -> Self {
        use crate::fluid::initial::initial_state;
        let g = &config.surface;
        let l = &config.liquid;
        let radius_sq = match config.domain.shape {
            DomainShape::Circular { radius } => (radius * radius) as f64,
            DomainShape::Full => -1.0,
        };
        Self {
            w: g.width,
            h: g.height,
            dx: (g.extent[0] / g.width as f32) as f64,
            dy: (g.extent[1] / g.height as f32) as f64,
            gh_base: (l.gravity * l.depth) as f64,
            sigma_h_rho: (l.surface_tension * l.depth / l.density) as f64,
            gamma: l.damping as f64,
            dt: config.solver.dt as f64,
            omega: (std::f64::consts::TAU * config.driving.frequency_hz as f64),
            accel_h: (config.driving.amplitude * l.depth) as f64,
            radius_sq,
            centre_x: (g.extent[0] * 0.5) as f64,
            centre_y: (g.extent[1] * 0.5) as f64,
            cur: initial_state(config)
                .into_iter()
                .map(|n| Node {
                    height: n.height as f64,
                    velocity_y: n.velocity_y as f64,
                })
                .collect(),
            next: vec![
                Node {
                    height: 0.0,
                    velocity_y: 0.0
                };
                (g.width * g.height) as usize
            ],
            time: 0.0,
        }
    }

    fn idx(&self, x: u32, y: u32) -> usize {
        (y * self.w + x) as usize
    }

    /// Dirichlet-aware sample — twin of WGSL `eta_at`.
    fn eta_at(&self, x: i64, y: i64) -> f64 {
        if x < 0 || y < 0 || x >= self.w as i64 || y >= self.h as i64 {
            return 0.0;
        }
        if self.radius_sq >= 0.0 {
            let px = x as f64 * self.dx;
            let py = y as f64 * self.dy;
            let ddx = px - self.centre_x;
            let ddy = py - self.centre_y;
            if ddx * ddx + ddy * ddy > self.radius_sq {
                return 0.0;
            }
        }
        self.cur[self.idx(x as u32, y as u32)].height
    }

    /// Twin of WGSL `lap`.
    fn lap(&self, x: i64, y: i64) -> f64 {
        let c = self.eta_at(x, y);
        let l = self.eta_at(x - 1, y);
        let r = self.eta_at(x + 1, y);
        let d = self.eta_at(x, y - 1);
        let u = self.eta_at(x, y + 1);
        (l + r - 2.0 * c) / (self.dx * self.dx) + (u + d - 2.0 * c) / (self.dy * self.dy)
    }

    fn inside_mask(&self, x: u32, y: u32) -> bool {
        if self.radius_sq < 0.0 {
            return true;
        }
        let px = x as f64 * self.dx;
        let py = y as f64 * self.dy;
        let ddx = px - self.centre_x;
        let ddy = py - self.centre_y;
        ddx * ddx + ddy * ddy <= self.radius_sq
    }

    /// One semi-implicit Euler step — twin of the WGSL `main` body.
    pub(crate) fn step(&mut self) {
        let gh_eff = self.gh_base + self.accel_h * (self.omega * self.time).cos();
        for y in 0..self.h {
            for x in 0..self.w {
                let xi = x as i64;
                let yi = y as i64;
                let lap_c = self.lap(xi, yi);
                let b = (self.lap(xi - 1, yi) + self.lap(xi + 1, yi) - 2.0 * lap_c)
                    / (self.dx * self.dx)
                    + (self.lap(xi, yi + 1) + self.lap(xi, yi - 1) - 2.0 * lap_c)
                        / (self.dy * self.dy);

                let node = self.cur[self.idx(x, y)];
                let accel = gh_eff * lap_c - self.sigma_h_rho * b - self.gamma * node.velocity_y;

                let mut v_new = node.velocity_y + self.dt * accel;
                let mut e_new = node.height + self.dt * v_new;

                if !self.inside_mask(x, y) {
                    v_new = 0.0;
                    e_new = 0.0;
                }

                let dst = self.idx(x, y);
                self.next[dst].height = e_new;
                self.next[dst].velocity_y = v_new;
            }
        }
        std::mem::swap(&mut self.cur, &mut self.next);
        self.time += self.dt;
    }

    /// Height of a single node — used to rebuild the strided output order
    /// (contract O1) in golden-file tests.
    pub(crate) fn height_at(&self, x: u32, y: u32) -> f64 {
        self.cur[self.idx(x, y)].height
    }
}
