# ADR-0011: Fluid model — parametrically forced damped wave equation

**Status:** Accepted (Phase 2)

> **In plain terms:** a liquid surface in a vibrating dish is simulated as a height field — a grid of points whose "altitude" evolves like a stretched membrane with real water physics (gravity, surface tension, damping). Shaking the dish up and down at the right frequency makes hexagonal or square ripples appear by themselves, just like on a CymaScope. We do not simulate the full 3D motion of the water; the height-field reduction is exactly what captures the Faraday pattern formation.

## Context

The design docs ([README](../../README.md), [ARCHITECTURE.md](../ARCHITECTURE.md)) listed the fluid module's ingredients as "Mathieu equation (Faraday instability) + incompressible Navier–Stokes + Laplace–Young surface tension", with output `Vec<FluidSurfaceNode>` (height + vertical velocity per mesh point) on grids from 512×512 to 2048×2048.

A height field `η(x,y)` is already a dimensionality reduction: full incompressible Navier–Stokes is intrinsically 3D and cannot live inside a 2D height representation. Claiming NS fidelity would be misleading; dropping it entirely is not a loss for the target phenomenon.

## Decision

Simulate the **linearized, damped wave equation with Mathieu-style parametric forcing**:

```
η_tt = h·g_eff(t)·∇²η − (σ·h/ρ)·∇⁴η − γ·η_t
g_eff(t) = g + a·cos(2π f t)
```

- `h` liquid depth, `σ` surface tension, `ρ` density, `γ` phenomenological damping.
- The gravity term uses shallow-water scaling (`c² = gh`); the Laplace–Young term
  `(σh/ρ)∇⁴η` reproduces the capillary dispersion branch `ω² = gh·k² + (σh/ρ)·k⁴`.
- Vertical shaking enters as a time-modulated restoring coefficient — the classic
  Mathieu form. This reproduces the two signatures that matter for cymatics:
  a genuine **Faraday threshold** (no pattern below critical drive) and
  **subharmonic response** (patterns oscillating at `f/2`).
- Discretization: regular Cartesian grid, fused 13-point biharmonic stencil
  (5-point Laplacian squared in one pass), semi-implicit Euler, Dirichlet rim via
  a domain mask (`Circular { radius }` default — faithful to the circular
  CymaScope dish — or `Full`). Neighbour reads force **ping-pong double buffering**
  with bind-group swap each step.
- Initial state: flat surface plus seeded white noise (shared `SplitMix64`),
  because the instability needs a seed to grow from.

## Alternatives considered

- **Full incompressible Navier–Stokes (3D)** — physically complete but requires a
  volumetric grid and pressure projection; orders of magnitude costlier and
  pointless for surface-pattern output. Rejected.
- **Stam "stable fluids" (velocity/density advection grid)** — excellent for smoke
  visuals, but its output is a transported scalar field, not a wave surface;
  it does not exhibit Faraday threshold behaviour without bolting on a height
  equation anyway. Rejected.
- **SPH particles** — matches the granular pipeline style but is far more expensive
  per "node" at 512²–2048² target scales and noisier for smooth surface fields.
  Rejected.

## Consequences

- The README/ARCHITECTURE wording "Navier–Stokes" must be corrected to describe
  this reduced model (done during Phase 2 docs sync).
- Phenomena outside linear surface waves (breaking, droplet ejection, mean-flow
  drift) are out of scope by construction.
- The stability precondition P4 (`dt·sqrt(gh·Λ₂ + (σh/ρ)·Λ₄) < 2`) is checkable
  eagerly at configuration validation — bad `dt` fails fast, never mid-simulation.
