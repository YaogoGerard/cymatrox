//! Public configuration surface of the fluid module.
//!
//! Mirrors `docs/CONTRACT.md` § Fluid: bench driving (`Driving`), physical
//! medium (`LiquidSpec`), discretization & reproducibility (`SurfaceGrid`),
//! active region (`DomainMask`) and numerical settings (`SolverParams`).
//! The physical model is decided in ADR-0011.

use crate::fluid::FluidError;

/// Grid dimension bounds per axis (README performance targets cap at 2048).
pub const MIN_GRID_DIM: u32 = 8;
pub const MAX_GRID_DIM: u32 = 2048;
/// Upper bound for `readback_stride`.
pub const MAX_STRIDE: u32 = 256;
/// Drive frequency bounds (contract P3).
pub(crate) const MIN_FREQUENCY_HZ: f32 = 0.1;
pub(crate) const MAX_FREQUENCY_HZ: f32 = 20_000.0;

/// Bench-level vertical vibration of the dish.
#[derive(Clone, Copy, Debug)]
pub struct Driving {
    /// Vibration frequency in Hz — live-tunable via `set_frequency`.
    pub frequency_hz: f32,
    /// Vertical acceleration amplitude a (m/s²) — live-tunable.
    pub amplitude: f32,
}

/// Physical description of the liquid and its environment.
#[derive(Clone, Copy, Debug)]
pub struct LiquidSpec {
    /// Density ρ (kg/m³).
    pub density: f32,
    /// Surface tension σ (N/m).
    pub surface_tension: f32,
    /// Liquid depth h (m).
    pub depth: f32,
    /// Phenomenological viscous damping γ (1/s).
    pub damping: f32,
    /// Gravitational acceleration g (m/s²).
    pub gravity: f32,
}

/// Discretization and reproducibility settings.
///
/// The buffer is always rectangular (`width × height` nodes spanning
/// `extent[0] × extent[1]` metres); the active domain is carved out by
/// [`DomainMask`].
#[derive(Clone, Copy, Debug)]
pub struct SurfaceGrid {
    pub width: u32,
    pub height: u32,
    /// Physical span `[extent_x, extent_y]` in metres.
    pub extent: [f32; 2],
    /// Return every Nth node per axis at readback; 1 = all nodes.
    /// Fixed at construction (contract I1).
    pub readback_stride: u32,
    /// Amplitude of the initial white-noise perturbation (m).
    pub noise_amplitude: f32,
    /// Seed of the shared deterministic RNG.
    pub seed: u64,
}

/// Active region within the grid buffer.
#[derive(Clone, Copy, Debug)]
pub enum DomainShape {
    /// Circular dish centred in the buffer (CymaScope-faithful default).
    Circular { radius: f32 },
    /// The whole rectangular grid is active.
    Full,
}

#[derive(Clone, Copy, Debug)]
pub struct DomainMask {
    pub shape: DomainShape,
}

/// Numerical settings.
#[derive(Clone, Copy, Debug)]
pub struct SolverParams {
    pub dt: f32,
}

/// Root configuration handed to [`crate::fluid::FluidSimulation::new`].
#[derive(Clone, Copy, Debug)]
pub struct FluidConfig {
    pub driving: Driving,
    pub liquid: LiquidSpec,
    pub surface: SurfaceGrid,
    pub domain: DomainMask,
    pub solver: SolverParams,
}

impl FluidConfig {
    /// Contract P1–P4, checked eagerly in `new()` (failure mode F1).
    pub(crate) fn validate(&self) -> Result<(), FluidError> {
        // P1 — grid geometry.
        let g = &self.surface;
        for (name, dim) in [("width", g.width), ("height", g.height)] {
            if !(MIN_GRID_DIM..=MAX_GRID_DIM).contains(&dim) {
                return Err(invalid(format!(
                    "P1 violated: surface.{name} = {dim} must be within \
                     {MIN_GRID_DIM}..={MAX_GRID_DIM}"
                )));
            }
        }
        for (i, &e) in g.extent.iter().enumerate() {
            if !e.is_finite() || e <= 0.0 {
                return Err(invalid(format!(
                    "P1 violated: surface.extent[{i}] = {e} must be finite and > 0"
                )));
            }
        }
        if !(1..=MAX_STRIDE).contains(&g.readback_stride) {
            return Err(invalid(format!(
                "P1 violated: readback_stride = {} must be within 1..={MAX_STRIDE}",
                g.readback_stride
            )));
        }
        if !g.noise_amplitude.is_finite() || g.noise_amplitude < 0.0 {
            return Err(invalid(
                "P1 violated: noise_amplitude must be finite and >= 0",
            ));
        }

        let dx = g.extent[0] / g.width as f32;
        let dy = g.extent[1] / g.height as f32;

        match self.domain.shape {
            DomainShape::Circular { radius } => {
                if !radius.is_finite() || radius <= 0.0 {
                    return Err(invalid(
                        "P1 violated: Circular radius must be finite and > 0",
                    ));
                }
                let fits =
                    radius <= dx * 0.5 * g.width as f32 && radius <= dy * 0.5 * g.height as f32;
                if !fits {
                    return Err(invalid(
                        "P1 violated: circular dish does not fit inside the extent",
                    ));
                }
            }
            DomainShape::Full => {}
        }

        // P2 — physical scalars.
        let l = &self.liquid;
        for (name, value, ok) in [
            (
                "density",
                l.density,
                l.density.is_finite() && l.density > 0.0,
            ),
            (
                "surface_tension",
                l.surface_tension,
                l.surface_tension.is_finite() && l.surface_tension >= 0.0,
            ),
            ("depth", l.depth, l.depth.is_finite() && l.depth > 0.0),
            (
                "damping",
                l.damping,
                l.damping.is_finite() && l.damping >= 0.0,
            ),
            (
                "gravity",
                l.gravity,
                l.gravity.is_finite() && l.gravity > 0.0,
            ),
            (
                "amplitude",
                self.driving.amplitude,
                self.driving.amplitude.is_finite() && self.driving.amplitude >= 0.0,
            ),
            (
                "dt",
                self.solver.dt,
                self.solver.dt.is_finite() && self.solver.dt > 0.0,
            ),
        ] {
            if !ok {
                return Err(invalid(format!(
                    "P2 violated: field `{name}` out of range (got {value})"
                )));
            }
        }

        // P3 — drive frequency range.
        let f = self.driving.frequency_hz;
        if !f.is_finite() || !(MIN_FREQUENCY_HZ..=MAX_FREQUENCY_HZ).contains(&f) {
            return Err(invalid(format!(
                "P3 violated: frequency_hz = {f} must be within \
                 [{MIN_FREQUENCY_HZ}, {MAX_FREQUENCY_HZ}] Hz"
            )));
        }

        // P4 — stability of semi-implicit Euler on the stiffest mode
        // (ADR-0011): dt · ω_max < 2 with ω_max² = gh·Λ₂ + (σh/ρ)·Λ₄.
        let lambda2 = 4.0 / (dx * dx) + 4.0 / (dy * dy);
        let lambda4 = lambda2 * lambda2;
        let sigma_h_rho = l.surface_tension * l.depth / l.density;
        let omega_max_sq = l.gravity * l.depth * lambda2 + sigma_h_rho * lambda4;
        if self.solver.dt * omega_max_sq.sqrt() >= 2.0 {
            return Err(invalid(format!(
                "P4 violated: dt = {} is above the CFL/stability bound {:.6} \
                 for this grid and liquid (ADR-0011)",
                self.solver.dt,
                2.0 / omega_max_sq.sqrt()
            )));
        }

        Ok(())
    }

    /// Output node counts after striding (contract O1 dimensions).
    pub(crate) fn output_dims(&self) -> (u32, u32) {
        let s = self.surface.readback_stride;
        (
            self.surface.width.div_ceil(s),
            self.surface.height.div_ceil(s),
        )
    }
}

fn invalid(msg: impl Into<String>) -> FluidError {
    FluidError::InvalidConfig(msg.into())
}
