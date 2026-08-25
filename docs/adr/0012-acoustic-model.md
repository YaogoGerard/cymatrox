# ADR-0012: Acoustic model — time-domain propagation with Gor'kov forces

**Status:** Accepted (Phase 3)

> **In plain terms:** the sound field is simulated as a real wave bouncing inside a closed rigid box. One entire wall vibrates like a speaker; over successive steps the standing wave builds up by interference of its own reflections — exactly how a levitator reaches steady state. At every grid point we then compute the net acoustic force on a small object (Gor'kov's formula): where that force pushes toward is where objects get trapped.

## Context

The design docs promise `Vec<AcousticPressureNode>` (pressure in Pa + force vector) on 64³–256³ grids via "Helmholtz equation + Gor'kov force potential" ([README](../../README.md), [ARCHITECTURE.md](../ARCHITECTURE.md)). The granular and fluid modules both expose `step()` as a *time advance*; an API user legitimately expects the same semantics here.

## Decision

**Time-domain explicit propagation**, first-order form:

```
p_t = q ;   q_t = c²∇²p        (semi-implicit Euler, fixed dt)
```

- **Transducer**: full face (axis + side configurable) imposing a Neumann condition
  `∂p/∂n ∝ ρ₀·ω·u₀·sin(2πft)` through mirrored ghost cells inside the stencil
  sampler; all other faces are rigid (`∂p/∂n = 0`). Total reflection guarantees
  standing-wave formation; transients are physical, not artefacts.
- **Cycle averaging without velocity storage**: per-node exponential moving averages
  of `p²` and `|∇p|²` with window τ = `averaging_periods/f`; particle velocity is
  recovered in steady regime as `⟨|v|²⟩ ≈ ⟨|∇p|²⟩/(ρ₀²ω²)` (Euler + phasor relation).
  Two scalars per node instead of a stored vector field — O(1) memory.
- **Gor'kov force** from the averaged fields:
  `U = V₀[f₁⟨p²⟩/(2ρ₀c₀²) − 3f₂⟨|∇p|²⟩/(4ρ₀²ω²)]`, `F = −∇U` by central differences.
  `f₁`, `f₂` are the compressibility/density contrasts of `ParticleSpec`; validity
  guarded eagerly by `R < λ/4` (clause P2).
- **Two dispatches per step** sharing one bind group: (1) wave update + EMA update
  (state ping-pong, mandatory for neighbour reads); (2) Gor'kov pass reading the
  just-written state buffer plus accumulators, writing the compacted strided output.
  Dispatches within one compute pass are ordered and memory-visible (WebGPU).
- **Initial state**: zero pressure plus seeded white noise (shared SplitMix64) —
  same rationale as the fluid module.

## Alternatives considered

- **Static modal superposition** (analytic box eigenmodes, granular-style) — exact
  steady field, but `step()` would have nothing to advance: breaks the temporal
  contract shared by all three modules and hides transient build-up. Rejected.
- **Yee-style FDTD** (staggered pressure/velocity grid) — marginal dispersion gain
  for a single homogeneous medium at unjustified two-grid complexity. Rejected.
- **Iterative frequency-domain Helmholtz solver** — fragile convergence near
  resonances, no dynamics. Rejected.

## Consequences

- Rigid walls everywhere ⇒ infinite reverberation; absorbing boundaries are an
  open point, not a v1 feature. Circular transducer patch likewise (v1.1).
- The steady-state field satisfies the Helmholtz equation, so the docs' wording
  remains truthful while the implementation stays honest about dynamics.
- CFL clause P4 (`dt·c·‖1/Δx‖₂ < 1`) is checkable eagerly at configuration
  validation — unstable setups fail fast, never mid-simulation.
