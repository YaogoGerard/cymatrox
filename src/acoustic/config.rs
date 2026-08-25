//! Public configuration surface of the acoustic module.
//!
//! Mirrors `docs/CONTRACT.md` § Acoustic: bench driving (`Driving`),
//! medium (`MediumSpec`), discretization (`VolumeGrid`), excitation
//! boundary (`TransducerSpec`), Gor'kov object (`ParticleSpec`) and
//! numerical settings (`SolverParams`). Model decided in ADR-0012.

use crate::acoustic::AcousticError;

/// Volume dimension bounds per axis (README performance targets cap at 256).
pub const MIN_GRID_DIM: u32 = 8;
pub const MAX_GRID_DIM: u32 = 256;
/// Upper bound for `readback_stride`.
pub const MAX_STRIDE: u32 = 64;
/// Drive frequency bounds (contract P3).
pub(crate) const MIN_FREQUENCY_HZ: f32 = 20.0;
pub(crate) const MAX_FREQUENCY_HZ: f32 = 40_000.0;

/// Bench-level drive signal of the transducer face.
#[derive(Clone, Copy, Debug)]
pub struct Driving {
    /// Drive frequency in Hz — live-tunable via `set_frequency`.
    pub frequency_hz: f32,
    /// Normal-velocity amplitude u₀ (m/s) — live-tunable.
    pub amplitude: f32,
}

/// Physical description of the gas filling the enclosure.
#[derive(Clone, Copy, Debug)]
pub struct MediumSpec {
    /// Density ρ₀ (kg/m³).
    pub density: f32,
    /// Sound speed c (m/s).
    pub sound_speed: f32,
}

/// Discretization and reproducibility settings.
#[derive(Clone, Copy, Debug)]
pub struct VolumeGrid {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    /// Physical span `[ex, ey, ez]` in metres.
    pub extent: [f32; 3],
    /// Return every Nth node per axis at readback; 1 = all nodes.
    /// Fixed at construction (contract I1).
    pub readback_stride: u32,
    /// Amplitude of the initial white-noise pressure perturbation (Pa).
    pub noise_amplitude: f32,
    /// Seed of the shared deterministic RNG.
    pub seed: u64,
}

/// Which wall the transducer occupies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Low,
    High,
}

/// v1 excitation: the full face vibrates uniformly
/// (Langevin-type levitator; ADR-0012).
#[derive(Clone, Copy, Debug)]
pub struct TransducerSpec {
    pub axis: Axis,
    pub side: Side,
}

/// The small object Gor'kov forces are evaluated for.
#[derive(Clone, Copy, Debug)]
pub struct ParticleSpec {
    /// Radius R (m) — must satisfy `R < λ/4` (contract P2).
    pub radius: f32,
    /// Density ρ_p (kg/m³).
    pub density: f32,
    /// Sound speed c_p (m/s).
    pub sound_speed: f32,
}

/// Numerical settings.
#[derive(Clone, Copy, Debug)]
pub struct SolverParams {
    pub dt: f32,
    /// EMA window in drive periods: τ = averaging_periods / f.
    pub averaging_periods: f32,
}

/// Root configuration handed to [`crate::acoustic::AcousticSimulation::new`].
#[derive(Clone, Copy, Debug)]
pub struct AcousticConfig {
    pub driving: Driving,
    pub medium: MediumSpec,
    pub volume: VolumeGrid,
    pub transducer: TransducerSpec,
    pub particle: ParticleSpec,
    pub solver: SolverParams,
}

impl AcousticConfig {
    /// Contract P1–P4, checked eagerly in `new()` (failure mode F1).
    pub(crate) fn validate(&self) -> Result<(), AcousticError> {
        // P1 — grid geometry.
        let g = &self.volume;
        for (name, dim) in [("width", g.width), ("height", g.height), ("depth", g.depth)] {
            if !(MIN_GRID_DIM..=MAX_GRID_DIM).contains(&dim) {
                return Err(invalid(format!(
                    "P1 violated: volume.{name} = {dim} must be within \
                     {MIN_GRID_DIM}..={MAX_GRID_DIM}"
                )));
            }
        }
        for (i, &e) in g.extent.iter().enumerate() {
            if !e.is_finite() || e <= 0.0 {
                return Err(invalid(format!(
                    "P1 violated: volume.extent[{i}] = {e} must be finite and > 0"
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

        // P3 first — the λ/4 guard below divides by frequency_hz.
        let f = self.driving.frequency_hz;
        if !f.is_finite() || !(MIN_FREQUENCY_HZ..=MAX_FREQUENCY_HZ).contains(&f) {
            return Err(invalid(format!(
                "P3 violated: frequency_hz = {f} must be within \
                 [{MIN_FREQUENCY_HZ}, {MAX_FREQUENCY_HZ}] Hz"
            )));
        }

        // P2 — physical scalars.
        let m = &self.medium;
        let p = &self.particle;
        let wavelength = m.sound_speed / f;
        for (name, value, ok) in [
            (
                "density",
                m.density,
                m.density.is_finite() && m.density > 0.0,
            ),
            (
                "sound_speed",
                m.sound_speed,
                m.sound_speed.is_finite() && m.sound_speed > 0.0,
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
            (
                "averaging_periods",
                self.solver.averaging_periods,
                self.solver.averaging_periods.is_finite() && self.solver.averaging_periods > 0.0,
            ),
            ("radius", p.radius, p.radius.is_finite() && p.radius > 0.0),
            (
                "particle.density",
                p.density,
                p.density.is_finite() && p.density > 0.0,
            ),
            (
                "particle.sound_speed",
                p.sound_speed,
                p.sound_speed.is_finite() && p.sound_speed > 0.0,
            ),
        ] {
            if !ok {
                return Err(invalid(format!(
                    "P2 violated: field `{name}` out of range (got {value})"
                )));
            }
        }
        // `radius` is already finite and > 0 (P2 loop above), so a plain
        // comparison is NaN-safe here.
        if p.radius >= 0.25 * wavelength {
            return Err(invalid(format!(
                "P2 violated: particle radius {} must satisfy R < λ/4 = {:.6} m \
                 at {:.0} Hz (Gor'kov validity, ADR-0012)",
                p.radius,
                0.25 * wavelength,
                f
            )));
        }

        // P4 — CFL stability of explicit Euler in 3D:
        // dt · c · ‖(1/dx, 1/dy, 1/dz)‖₂ < 1.
        let dx = g.extent[0] / g.width as f32;
        let dy = g.extent[1] / g.height as f32;
        let dz = g.extent[2] / g.depth as f32;
        let inv_sq = 1.0 / (dx * dx) + 1.0 / (dy * dy) + 1.0 / (dz * dz);
        if self.solver.dt * m.sound_speed * inv_sq.sqrt() >= 1.0 {
            return Err(invalid(format!(
                "P4 violated: dt = {} is above the CFL bound {:.3e} s \
                 for this grid and medium (ADR-0012)",
                self.solver.dt,
                1.0 / (m.sound_speed * inv_sq.sqrt())
            )));
        }

        Ok(())
    }

    /// Output node counts after striding (contract O1 dimensions).
    pub(crate) fn output_dims(&self) -> (u32, u32, u32) {
        let s = self.volume.readback_stride;
        (
            self.volume.width.div_ceil(s),
            self.volume.height.div_ceil(s),
            self.volume.depth.div_ceil(s),
        )
    }
}

fn invalid(msg: impl Into<String>) -> AcousticError {
    AcousticError::InvalidConfig(msg.into())
}
