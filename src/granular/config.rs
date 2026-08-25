//! Public configuration surface of the granular module.
//!
//! Structure follows the validated contract (`docs/CONTRACT.md` § Granular):
//! bench knobs (`Driving`), physical medium (`PlateSpec`), grain population
//! (`GrainBed`) and numerical settings (`SolverParams`). Mode-source
//! semantics are decided in ADR-0009.

use crate::granular::GranularError;

/// Maximum supported grain population (README performance targets).
pub const MAX_GRAINS: u32 = 1_000_000;
/// Audible range — the cymatics domain (contract P3).
pub(crate) const MIN_FREQUENCY_HZ: f32 = 20.0;
pub(crate) const MAX_FREQUENCY_HZ: f32 = 20_000.0;

/// How mode shapes/frequencies reach the solver (ADR-0009).
#[derive(Clone, Debug)]
pub enum ModeSelection {
    /// Shape indices derived live from `Driving::frequency_hz` via the
    /// idealized scaling; recomputed on every retune.
    Auto,
    /// Fixed user-chosen shapes, driven at `frequency_hz`.
    Explicit(Vec<(u32, u32)>),
    /// User-measured resonances; the entry closest to `frequency_hz`
    /// is selected live.
    Measured(Vec<EigenPair>),
}

/// One user-measured plate resonance (ADR-0009).
#[derive(Clone, Copy, Debug)]
pub struct EigenPair {
    pub m: u32,
    pub n: u32,
    /// Measured resonance frequency in Hz.
    pub omega_measured_hz: f32,
}

/// Bench-level driving signal (contract Tier 1).
#[derive(Clone, Debug)]
pub struct Driving {
    pub frequency_hz: f32,
    pub amplitude: f32,
    pub modes: ModeSelection,
}

/// Physical description of the vibrating medium (contract Tier 2).
///
/// Material properties (`PlateSpec::Material`) are deferred — see the
/// contract open points.
#[derive(Clone, Copy, Debug)]
pub enum PlateSpec {
    Idealized { side: f32 },
}

/// Grain population and reproducibility settings.
#[derive(Clone, Copy, Debug)]
pub enum InitialDistribution {
    Uniform,
    CenteredCluster,
    Grid,
}

#[derive(Clone, Copy, Debug)]
pub struct GrainBed {
    pub count: u32,
    pub distribution: InitialDistribution,
    pub seed: u64,
}

/// Numerical settings — instrument parameters, not physics (Tier 3).
#[derive(Clone, Copy, Debug)]
pub struct SolverParams {
    pub dt: f32,
    pub drag: f32,
    pub restitution: f32,
    pub coupling_k: f32,
    /// Frequency of mode (1,1) in the idealized scaling; used only by `Auto`.
    pub base_frequency_hz: f32,
}

/// Root configuration handed to [`crate::granular::GranularSimulation::new`].
#[derive(Clone, Debug)]
pub struct GranularConfig {
    pub experiment: Driving,
    pub medium: PlateSpec,
    pub grains: GrainBed,
    pub solver: SolverParams,
}

/// One resolved mode entry actually uploaded to the GPU each step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ResolvedMode {
    pub m: u32,
    pub n: u32,
    pub omega_hz: f32,
}

impl GranularConfig {
    /// Contract P1–P4, checked eagerly in `new()` (failure mode F1).
    pub(crate) fn validate(&self) -> Result<(), GranularError> {
        // P1 — population bounds.
        if self.grains.count == 0 || self.grains.count > MAX_GRAINS {
            return Err(invalid(format!(
                "P1 violated: grains.count = {} must be within 1..={} \
                 (see README performance targets)",
                self.grains.count, MAX_GRAINS
            )));
        }

        // P2 — finite, physically meaningful scalars.
        let s = &self.solver;
        let side_ok = match self.medium {
            PlateSpec::Idealized { side } => side.is_finite() && side > 0.0,
        };
        if !side_ok {
            return Err(invalid("P2 violated: medium side must be finite and > 0"));
        }
        for (name, value, ok) in [
            (
                "frequency-related amplitude",
                self.experiment.amplitude,
                self.experiment.amplitude.is_finite() && self.experiment.amplitude >= 0.0,
            ),
            ("dt", s.dt, s.dt.is_finite() && s.dt > 0.0),
            ("drag", s.drag, s.drag.is_finite() && s.drag >= 0.0),
            (
                "coupling_k",
                s.coupling_k,
                s.coupling_k.is_finite() && s.coupling_k >= 0.0,
            ),
            (
                "base_frequency_hz",
                s.base_frequency_hz,
                s.base_frequency_hz.is_finite() && s.base_frequency_hz > 0.0,
            ),
        ] {
            if !ok {
                return Err(invalid(format!(
                    "P2 violated: solver/experiment field `{name}` out of range (got {value})"
                )));
            }
        }
        if !(0.0..=1.0).contains(&s.restitution) || !s.restitution.is_finite() {
            return Err(invalid(
                "P2 violated: restitution must be within [0.0, 1.0]",
            ));
        }

        // P3 — audible range.
        let f = self.experiment.frequency_hz;
        if !f.is_finite() || !(MIN_FREQUENCY_HZ..=MAX_FREQUENCY_HZ).contains(&f) {
            return Err(invalid(format!(
                "P3 violated: frequency_hz = {f} must be within [{MIN_FREQUENCY_HZ}, {MAX_FREQUENCY_HZ}] Hz"
            )));
        }

        // P4 — mode lists well-formed.
        match &self.experiment.modes {
            ModeSelection::Auto => {}
            ModeSelection::Explicit(list) => {
                if list.is_empty() {
                    return Err(invalid("P4 violated: Explicit mode list must not be empty"));
                }
                if list.iter().any(|&(m, n)| m == 0 || n == 0) {
                    return Err(invalid("P4 violated: mode indices must be >= 1"));
                }
            }
            ModeSelection::Measured(list) => {
                if list.is_empty() {
                    return Err(invalid(
                        "P4 violated: Measured eigenpair list must not be empty",
                    ));
                }
                for p in list {
                    if p.m == 0
                        || p.n == 0
                        || p.omega_measured_hz <= 0.0
                        || !p.omega_measured_hz.is_finite()
                    {
                        return Err(invalid(
                            "P4 violated: EigenPair requires indices >= 1 and omega_measured_hz > 0",
                        ));
                    }
                }
            }
        }

        Ok(())
    }

    /// Resolve the current mode table per ADR-0009 semantics.
    ///
    /// Called every step so that `Auto`/`Measured` retuning takes effect
    /// without any buffer reallocation (contract I1).
    pub(crate) fn resolve_modes(&self) -> Vec<ResolvedMode> {
        let f = self.experiment.frequency_hz;
        match &self.experiment.modes {
            ModeSelection::Explicit(list) => list
                .iter()
                .map(|&(m, n)| ResolvedMode { m, n, omega_hz: f })
                .collect(),
            ModeSelection::Auto => {
                // Ideal scaling: omega_mn = base * (m^2 + n^2) / 2.
                let base = self.solver.base_frequency_hz;
                let mut best = (1u32, 1u32);
                let mut best_err = f32::INFINITY;
                for m in 1..=24u32 {
                    for n in 1..=24u32 {
                        let err = (base * (m * m + n * n) as f32 * 0.5 - f).abs();
                        // Tie-break toward lower total order.
                        if err < best_err - f32::EPSILON
                            || ((err - best_err).abs() <= f32::EPSILON && m + n < best.0 + best.1)
                        {
                            best_err = err;
                            best = (m, n);
                        }
                    }
                }
                vec![ResolvedMode {
                    m: best.0,
                    n: best.1,
                    omega_hz: f,
                }]
            }
            ModeSelection::Measured(list) => {
                let pick = list
                    .iter()
                    .enumerate()
                    .min_by(|(_, a), (_, b)| {
                        let da = (a.omega_measured_hz - f).abs();
                        let db = (b.omega_measured_hz - f).abs();
                        da.total_cmp(&db)
                    })
                    .expect("validated non-empty");
                let p = list[pick.0];
                vec![ResolvedMode {
                    m: p.m,
                    n: p.n,
                    omega_hz: p.omega_measured_hz,
                }]
            }
        }
    }
}

fn invalid(msg: impl Into<String>) -> GranularError {
    GranularError::InvalidConfig(msg.into())
}
