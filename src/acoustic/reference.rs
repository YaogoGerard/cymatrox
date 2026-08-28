//! Double-precision CPU oracle mirroring `acoustic.wgsl` line for line
//! ([ADR-0004](../../../docs/adr/0004-numerical-precision-strategy.md),
//! [ADR-0007](../../../docs/adr/0007-test-strategy.md)).
//!
//! Test-only (`cfg(any(test, feature = "reference"))`). Operation order
//! deliberately matches the shader — same ghost-cell handling, same
//! stencil traversal, same EMA update — so drift stays within the
//! contract tolerance instead of compounding through reordering.
//!
//! This oracle is intentionally not part of the runtime path; it exists as a
//! validation aid and may legitimately contain helper methods that are only
//! exercised in golden-file tests.

#![allow(dead_code)]

use crate::acoustic::config::{AcousticConfig, Axis, Side};

#[derive(Clone, Copy)]
struct State {
    p: f64,
    q: f64,
}

#[derive(Clone, Copy)]
struct Avg {
    p2: f64,
    g2: f64,
}

pub(crate) struct ReferenceSim {
    nx: u32,
    ny: u32,
    nz: u32,
    dx: f64,
    dy: f64,
    dz: f64,
    c2: f64,
    dt: f64,
    omega: f64,
    ema_alpha: f64,
    neumann_amp: f64,
    gk_p_coeff: f64,
    gk_g_coeff: f64,
    axis_is_x: bool,
    axis_is_y: bool,
    side_high: bool,
    cur: Vec<State>,
    avg: Vec<Avg>,
    time: f64,
}

impl ReferenceSim {
    pub(crate) fn new(config: &AcousticConfig) -> Self {
        use crate::acoustic::initial::initial_state;
        let g = &config.volume;
        let m = &config.medium;
        let par = &config.particle;
        let s = &config.solver;
        let f = config.driving.frequency_hz as f64;
        let rho0 = m.density as f64;
        let c = m.sound_speed as f64;

        // Gor'kov contrasts (ADR-0012).
        let v0 = 4.0 / 3.0 * std::f64::consts::PI * (par.radius as f64).powi(3);
        let f1 = 1.0 - rho0 * c * c / ((par.density * par.sound_speed) as f64).powi(2);
        let f2 = 2.0 * ((par.density - m.density) as f64)
            / (2.0 * (par.density as f64) + (m.density as f64));
        let omega = std::f64::consts::TAU * f;
        let tau = s.averaging_periods as f64 / f;

        let (axis_is_x, axis_is_y, side_high) =
            match (&config.transducer.axis, &config.transducer.side) {
                (Axis::X, Side::Low) => (true, false, false),
                (Axis::X, Side::High) => (true, false, true),
                (Axis::Y, Side::Low) => (false, true, false),
                (Axis::Y, Side::High) => (false, true, true),
                (Axis::Z, Side::Low) => (false, false, false),
                (Axis::Z, Side::High) => (false, false, true),
            };

        Self {
            nx: g.width,
            ny: g.height,
            nz: g.depth,
            dx: (g.extent[0] / g.width as f32) as f64,
            dy: (g.extent[1] / g.height as f32) as f64,
            dz: (g.extent[2] / g.depth as f32) as f64,
            c2: c * c,
            dt: s.dt as f64,
            omega,
            ema_alpha: 1.0 - (-((s.dt as f64) / tau)).exp(),
            neumann_amp: rho0 * omega * (config.driving.amplitude as f64),
            gk_p_coeff: v0 * f1 / (2.0 * rho0 * c * c),
            gk_g_coeff: -v0 * 3.0 * f2 / (4.0 * rho0 * rho0 * omega * omega),
            axis_is_x,
            axis_is_y,
            side_high,
            cur: initial_state(config)
                .into_iter()
                .map(|(p, q)| State {
                    p: p as f64,
                    q: q as f64,
                })
                .collect(),
            avg: vec![Avg { p2: 0.0, g2: 0.0 }; (g.width * g.height * g.depth) as usize],
            time: 0.0,
        }
    }

    fn idx(&self, x: u32, y: u32, z: u32) -> usize {
        ((z * self.ny + y) * self.nx + x) as usize
    }

    fn forcing(&self) -> f64 {
        self.neumann_amp * (self.omega * self.time).sin()
    }

    /// Twin of WGSL `is_driven_face`: true iff the reflection happens
    /// along the transducer axis AND on the transducer side.
    fn driven_face(&self, axis_is_x: bool, axis_is_y: bool, coord: i64, limit: i64) -> bool {
        if self.axis_is_x != axis_is_x || self.axis_is_y != axis_is_y {
            return false;
        }
        if coord < 0 {
            !self.side_high
        } else if coord >= limit {
            self.side_high
        } else {
            false
        }
    }

    /// Twin of WGSL `p_at`.
    fn p_at(&self, mut x: i64, mut y: i64, mut z: i64) -> f64 {
        let (nx, ny, nz) = (self.nx as i64, self.ny as i64, self.nz as i64);
        // Exactly one of x/y/z may be outside — 7-point stencil guarantee.
        let mut refl_x = false;
        let mut refl_y = false;
        let (coord, limit): (i64, i64) = if x < 0 || x >= nx {
            refl_x = true;
            (x, nx)
        } else if y < 0 || y >= ny {
            refl_y = true;
            (y, ny)
        } else if z < 0 || z >= nz {
            (z, nz)
        } else {
            return self.cur[self.idx(x as u32, y as u32, z as u32)].p;
        };
        let inner = if coord < 0 { 1 } else { limit - 2 };
        // Redirect to the mirrored inner node along the offending axis.
        if refl_x {
            x = inner;
        } else if refl_y {
            y = inner;
        } else {
            z = inner;
        }
        let base = self.cur[self.idx(x as u32, y as u32, z as u32)].p;
        if self.driven_face(refl_x, refl_y, coord, limit) {
            let sgn = if coord < 0 { -1.0 } else { 1.0 };
            let spacing = if refl_x {
                self.dx
            } else if refl_y {
                self.dy
            } else {
                self.dz
            };
            base - sgn * 2.0 * spacing * self.forcing()
        } else {
            base
        }
    }

    /// Twin of the `wave_step` body over all nodes.
    pub(crate) fn step(&mut self) {
        let alpha = self.ema_alpha;
        let keep = 1.0 - alpha;
        let mut next = vec![State { p: 0.0, q: 0.0 }; self.cur.len()];

        for z in 0..self.nz {
            for y in 0..self.ny {
                for x in 0..self.nx {
                    let xi = x as i64;
                    let yi = y as i64;
                    let zi = z as i64;
                    let c_node = self.p_at(xi, yi, zi);
                    let lap = (self.p_at(xi - 1, yi, zi) + self.p_at(xi + 1, yi, zi)
                        - 2.0 * c_node)
                        / (self.dx * self.dx)
                        + (self.p_at(xi, yi - 1, zi) + self.p_at(xi, yi + 1, zi) - 2.0 * c_node)
                            / (self.dy * self.dy)
                        + (self.p_at(xi, yi, zi - 1) + self.p_at(xi, yi, zi + 1) - 2.0 * c_node)
                            / (self.dz * self.dz);

                    let st = self.cur[self.idx(x, y, z)];
                    let q_new = st.q + self.dt * self.c2 * lap;
                    let p_new = st.p + self.dt * q_new;

                    next[self.idx(x, y, z)] = State { p: p_new, q: q_new };

                    let gx =
                        (self.p_at(xi + 1, yi, zi) - self.p_at(xi - 1, yi, zi)) / (2.0 * self.dx);
                    let gy =
                        (self.p_at(xi, yi + 1, zi) - self.p_at(xi, yi - 1, zi)) / (2.0 * self.dy);
                    let gz =
                        (self.p_at(xi, yi, zi + 1) - self.p_at(xi, yi, zi - 1)) / (2.0 * self.dz);
                    let old = self.avg[self.idx(x, y, z)];
                    let i = self.idx(x, y, z);
                    self.avg[i] = Avg {
                        p2: keep * old.p2 + alpha * p_new * p_new,
                        g2: keep * old.g2 + alpha * (gx * gx + gy * gy + gz * gz),
                    };
                }
            }
        }
        self.cur = next;
        self.time += self.dt;
    }

    /// Twin of the `gorkov` body at one node (clamped neighbours).
    pub(crate) fn node_at(&self, x: u32, y: u32, z: u32) -> (f64, [f64; 3]) {
        let u_at = |x: u32, y: u32, z: u32| -> f64 {
            let a = &self.avg[self.idx(x, y, z)];
            self.gk_p_coeff * a.p2 + self.gk_g_coeff * a.g2
        };
        let mx = (self.nx - 1) as i64;
        let my = (self.ny - 1) as i64;
        let mz = (self.nz - 1) as i64;
        let xp = (x as i64 + 1).min(mx) as u32;
        let xm = (x as i64 - 1).max(0) as u32;
        let yp = (y as i64 + 1).min(my) as u32;
        let ym = (y as i64 - 1).max(0) as u32;
        let zp = (z as i64 + 1).min(mz) as u32;
        let zm = (z as i64 - 1).max(0) as u32;

        let fx = -(u_at(xp, y, z) - u_at(xm, y, z)) / (((xp - xm) as f64) * self.dx);
        let fy = -(u_at(x, yp, z) - u_at(x, ym, z)) / (((yp - ym) as f64) * self.dy);
        let fz = -(u_at(x, y, zp) - u_at(x, y, zm)) / (((zp - zm) as f64) * self.dz);
        (self.cur[self.idx(x, y, z)].p, [fx, fy, fz])
    }
}
