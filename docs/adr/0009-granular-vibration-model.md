# ADR-0009: Granular vibration model and mode sources

**Status:** Accepted

> **In plain terms:** the plate vibrates as a sum of simple wave shapes ("modes"); scientists can let Cymatrox pick them automatically, pick them by hand, or — crucially — inject frequencies they actually measured on their own plate in the lab.

## Context

The granular module (Phase 1, Chladni plates) needs a concrete vibration model and must decide how mode shapes/frequencies reach the solver. Two constraints shaped this decision: the target users are experimentalists who trust measured data over ideal models, and Phase 1 must stay implementable without a full anisotropic plate solver.

## Decision

1. **Vibration model** — modal superposition on an ideal simply-supported square plate:
   `w(x,y,t) = Σ A·sin(mπx/L)·sin(nπy/L)·cos(ω t)` with `ωₘₙ = ω_base·(m²+n²)/2` (Sophie Germain bending-wave scaling). Grain driving force `F = −k·∇(|w|²)` toward nodal lines; linear drag; wall restitution; semi-implicit Euler at fixed `dt`.
2. **Three mode sources** (`ModeSelection`):
   - `Auto` — shape indices derived live from the requested frequency;
   - `Explicit(Vec<(u32,u32)>)` — user-picked shapes driven at the requested frequency;
   - `Measured(Vec<EigenPair>)` — user-measured resonances injected into the simulation; the entry closest to the current frequency is selected live.
   All sources share identical semantics so GPU and reference paths stay comparable ([CONTRACT.md](../CONTRACT.md), invariant I2).
3. **Material properties deferred** — `PlateSpec::Material { E, h, ρ, ν }` (full Germain bending stiffness) is recorded as a contract open point, not implemented in Phase 1.

## Alternatives considered

- **Single hard-coded auto mode** — simplest, but turns the library into a toy; experimenters need explicit control.
- **Full material solver from day one** — scientifically complete but high-risk for a first pipeline; modal superposition already reproduces canonical Chladni patterns; deferred via contract open point instead.
- **Frequency sweeps as a dedicated API** — rejected: `set_frequency()` per step composes into a sweep with zero extra API surface; documented pattern rather than abstraction.

## Consequences

- The solver consumes a *list of `(shape, ω)` entries* regardless of source — one shader path for all three selections.
- `Measured` gives Cymatrox its differentiating feature: simulations anchored to real lab data, aligned with the "physics-first" positioning.
- Adding `PlateSpec::Material` later only changes how ωₘₙ values are produced, not the solver structure.
