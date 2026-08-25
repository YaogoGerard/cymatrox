# Contract & Invariants

This document lists the preconditions, postconditions, and invariants each module guarantees. It's part of the "Contract & Invariants before code" step of the design process, and is meant to be filled in per module as it's implemented — not written from the API after the fact.

## How to fill this in, per module

For each public function/struct, specify:

- **Preconditions** — what must be true of the input for the call to be valid (e.g. grid dimensions > 0, frequency within a physically meaningful range).
- **Postconditions** — what's guaranteed true of the output on success (e.g. output `Vec` length always equals grid resolution).
- **Invariants** — what stays true across the object's lifetime (e.g. `GpuContext` device handle never becomes invalid while the struct lives).
- **Failure modes** — what causes an `Err` variant, and which one.
- **Golden-file tolerance** — required for every module ([ADR-0007](./adr/0007-test-strategy.md)): the numerical tolerance used when comparing GPU (`f32`) output against the CPU `f64` reference implementation.

## Granular module

_Status: **Accepted** — validated before implementation (Phase 1)._

### Public surface

```rust
pub struct GranularConfig {
    pub experiment: Driving,       // bench knobs (Tier 1)
    pub medium:     PlateSpec,     // physical description (Tier 2)
    pub grains:     GrainBed,      // population & reproducibility
    pub solver:     SolverParams,  // numerical settings (Tier 3)
}

pub struct Driving {
    pub frequency_hz: f32,          // live-tunable
    pub amplitude: f32,             // live-tunable
    pub modes: ModeSelection,
}

pub enum ModeSelection {
    /// Mode shape derived from `frequency_hz` via the idealized plate scaling.
    Auto,
    /// Fixed mode shapes chosen by the user; driven at `frequency_hz`.
    Explicit(Vec<(u32, u32)>),
    /// User-measured resonances; the entry closest to `frequency_hz`
    /// is selected live on every retune.
    Measured(Vec<EigenPair>),
}
pub struct EigenPair { pub m: u32, pub n: u32, pub omega_measured_hz: f32 }

pub enum PlateSpec {
    /// Ideal simply-supported square plate.
    Idealized { side: f32 },
    // Material { .. } — deferred, see open points.
}

pub struct GrainBed {
    pub count: u32,
    pub distribution: InitialDistribution, // Uniform | CenteredCluster | Grid
    pub seed: u64,
}

pub struct SolverParams {
    pub dt: f32,
    pub drag: f32,
    pub restitution: f32,
    pub coupling_k: f32,
    /// Frequency of mode (1,1) in the idealized scaling — used only by `Auto`.
    pub base_frequency_hz: f32,
}

pub struct GranularData {           // GPU ↔ CPU layout, mirrored + asserted
    pub position: [f32; 2],
    pub velocity: [f32; 2],
}

impl GranularSimulation {
    pub fn new(ctx: &GpuContext, config: GranularConfig) -> Result<Self>;
    pub fn set_frequency(&mut self, frequency_hz: f32);   // no reallocation
    pub fn set_amplitude(&mut self, amplitude: f32);      // no reallocation
    pub fn step(&mut self) -> Result<Vec<GranularData>>;  // blocking (ADR-0006)
}
```

### Physics model

- Square plate, side `L`, ideal simply-supported boundaries (`PlateSpec::Idealized`).
- Vibration field by modal superposition of the selected modes:
  `w(x,y,t) = Σ A·sin(mπx/L)·sin(nπy/L)·cos(ω t)` with ideal scaling `ωₘₙ = ω_base·(m²+n²)/2`.
- Grain driving force: `F = −k·∇(|w|²)` — grains migrate toward nodal lines.
- Linear drag `−γv`; wall rebounds with restitution `r`.
- Integration: semi-implicit Euler at fixed `dt`, deterministic given `seed`.

### Precondition semantics for mode sources

- `Auto` → shape indices `(m,n)` minimizing `|ωₘₙ − frequency_hz|`, recomputed on every `set_frequency`; drive at `frequency_hz`.
- `Explicit(list)` → shapes fixed; drive at `frequency_hz`; list non-empty.
- `Measured(list)` → the `EigenPair` whose measured ω is closest to `frequency_hz`; recomputed live; list non-empty.

### Preconditions

- **P1** — `grains.count` in `1..=1_000_000`.
- **P2** — all floats finite and: `side > 0`, `amplitude ≥ 0`, `dt > 0`, `drag ≥ 0`, `coupling_k ≥ 0`, `restitution ∈ [0,1]`, `base_frequency_hz > 0`.
- **P3** — `frequency_hz ∈ [20.0, 20_000.0]` (audible range — cymatics domain).
- **P4** — `Explicit`/`Measured` lists non-empty; mode indices `m, n ≥ 1`; measured ω > 0.

### Postconditions

- **O1** — every `step()` returns exactly `count` elements, in stable order.
- **O2** — all returned positions lie within `[0, L]²` (walls enforced post-integration).
- **O3** — `step()` returns only after GPU completion and CPU copy ([ADR-0006](./adr/0006-gpu-cpu-readback-strategy.md)).
- **O4** — same `(config, seed)` ⇒ bit-identical trajectory across runs on the same adapter.

### Invariants

- **I1** — buffers allocated once at construction; only `frequency_hz`/`amplitude` are live-mutable (uniform rewrite, never reallocation).
- **I2** — GPU and reference implementations share identical mode-source semantics (see above).

### Failure modes

- **F1** — any violated precondition → `Error::Granular(GranularError::InvalidConfig)` (checked eagerly in `new()`, message names the clause).
- **F2** — readback map/poll failure → `Error::Gpu(..)` ([ADR-0006](./adr/0006-gpu-cpu-readback-strategy.md)).

### Golden-file tolerance (per [ADR-0007](./adr/0007-test-strategy.md))

After 100 steps on the reference config (4096 grains, seed 123, `Auto` modes at 440 Hz), mean positional deviation between GPU (`f32`) output and the CPU (`f64`) reference must stay below `1e-3 · side`. **Frozen**: measured drift on the first golden-file run was well under budget; enforced by `granular::sim::tests::golden_file_within_tolerance`.

### Open points

- `PlateSpec::Material { youngs_modulus, thickness, density, poisson }` — full Sophie Germain bending-wave solver; deferred until `Idealized` ships and proves the pipeline.

## Fluid module

_Status: **Accepted** — validated before implementation (Phase 2)._

### Public surface

```rust
pub struct FluidConfig {
    pub driving: Driving,        // bench knobs (Tier 1)
    pub liquid:  LiquidSpec,     // physical description (Tier 2)
    pub surface: SurfaceGrid,    // discretization & reproducibility
    pub domain:  DomainMask,     // active region within the grid
    pub solver:  SolverParams,   // numerical settings (Tier 3)
}

pub struct Driving {
    pub frequency_hz: f32,  // vertical vibration frequency — live-tunable
    pub amplitude: f32,     // vertical acceleration amplitude (m/s²) — live-tunable
}

pub struct LiquidSpec {
    pub density: f32,          // ρ (kg/m³)
    pub surface_tension: f32,  // σ (N/m)
    pub depth: f32,            // h (m), shallow-water height field
    pub damping: f32,          // γ (1/s), phenomenological viscous damping
    pub gravity: f32,          // g (m/s²)
}

/// Physical span of the rectangular buffer (metres).
/// Grid spacing: dx = extent[0]/width, dy = extent[1]/height.
pub struct SurfaceGrid {
    pub width: u32,
    pub height: u32,
    pub extent: [f32; 2],
    /// Returns every Nth node per axis at readback (default 1 = all nodes).
    /// Fixed at construction — changing it reallocates by design.
    pub readback_stride: u32,
    /// Amplitude of the initial white-noise perturbation (m).
    pub noise_amplitude: f32,
    pub seed: u64,
}

pub enum DomainShape {
    /// Circular dish centred in the buffer (CymaScope-faithful default).
    Circular { radius: f32 },
    /// The whole rectangular grid is active.
    Full,
}
pub struct DomainMask { pub shape: DomainShape }

pub struct SolverParams {
    pub dt: f32,
}

pub struct FluidSurfaceNode {       // GPU ↔ CPU layout, mirrored + asserted
    pub height: f32,                // η (m)
    pub velocity_y: f32,            // ∂η/∂t (m/s)
}

impl FluidSimulation {
    pub fn new(ctx: &GpuContext, config: FluidConfig) -> Result<Self>;
    pub fn set_frequency(&mut self, frequency_hz: f32);   // no reallocation
    pub fn set_amplitude(&mut self, amplitude: f32);      // no reallocation
    pub fn step(&mut self) -> Result<Vec<FluidSurfaceNode>>;  // blocking (ADR-0006)
}
```

### Physics model

- Height field `η(x,y,t)` on a regular grid (`dx = extent[0]/width`, `dy = extent[1]/height`).
- Damped wave equation with Mathieu-style parametric forcing ([ADR-0011](./adr/0011-fluid-model.md)):
  `η_tt = h·g_eff(t)·∇²η − (σ·h/ρ)·∇⁴η − γ·η_t`, with `g_eff(t) = g + amplitude·cos(2π·frequency_hz·t)`.
  The `−(σh/ρ)∇⁴` term is the Laplace–Young surface tension (dispersion `ω² = gh·k² + (σh/ρ)·k⁴`);
  the time-modulated gravity term reproduces the Faraday threshold and subharmonic response.
- Biharmonic applied as a fused 13-point stencil (5-point Laplacian squared, single pass).
- Integration: semi-implicit Euler at fixed `dt`; Dirichlet rim — nodes outside the
  domain mask are pinned at `η = 0, v = 0` every step.
- Initial state: flat surface + white noise of `noise_amplitude`, seeded by the shared
  deterministic RNG (`SplitMix64`) — the instability needs a seed to grow from.

### Preconditions

- **P1** — `width, height ∈ [8, 2048]`; `extent[i] > 0` and finite; `Circular.radius > 0`
  and the disc fits inside the extent; `readback_stride ∈ [1, 256]`;
  `noise_amplitude ≥ 0`.
- **P2** — all floats finite and: `density > 0`, `surface_tension ≥ 0`, `depth > 0`,
  `damping ≥ 0`, `gravity > 0`, `dt > 0`, `amplitude ≥ 0`.
- **P3** — `frequency_hz ∈ [0.1, 20_000.0]` Hz.
- **P4** — numerical stability of semi-implicit Euler on the stiffest mode:
  `dt · sqrt(g·h·Λ₂ + (σh/ρ)·Λ₄) < 2`, with `Λ₂ = 4/dx² + 4/dy²`, `Λ₄ = Λ₂²`.

### Postconditions

- **O1** — every `step()` returns exactly `ceil(width/stride) · ceil(height/stride)`
  elements, in stable row-major order over the kept nodes.
- **O2** — all returned heights are finite; nodes outside the domain mask report
  exactly `η = 0, v = 0`.
- **O3** — `step()` returns only after GPU completion and CPU copy ([ADR-0006](./adr/0006-gpu-cpu-readback-strategy.md)).
- **O4** — same `(config, seed)` ⇒ bit-identical trajectory across runs on the same adapter.

### Invariants

- **I1** — buffers allocated once at construction (including the strided staging
  buffer); only `frequency_hz`/`amplitude` are live-mutable (uniform rewrite).
- **I2** — GPU and reference implementations share identical stencil weights,
  forcing phase and update order.
- **I3** — neighbour reads require double buffering: state ping-pongs between two
  storage buffers via bind-group swap; no aliasing hazard ever reaches the shader.

### Failure modes

- **F1** — any violated precondition → `Error::Fluid(FluidError::InvalidConfig)`
  (checked eagerly in `new()`, message names the clause).
- **F2** — readback map/poll failure → `Error::Gpu(..)` ([ADR-0006](./adr/0006-gpu-cpu-readback-strategy.md)).

### Golden-file tolerance (per [ADR-0007](./adr/0007-test-strategy.md))

After 100 steps on the reference config (96×96 grid, `Circular` dish r = 25 mm,
60 Hz drive), mean `|Δη|` between GPU (`f32`) output and the CPU (`f64`) reference,
averaged over returned nodes, must stay below `1e-11 m`. **Frozen**: the first
golden-file run measured ≈ `2.9e-13 m`; enforced by
`fluid::sim::tests::golden_file_within_tolerance`.

### Open points

- Frequency-dependent damping calibration against a physical CymaScope trace.


## Acoustic module

_Status: **Accepted** — validated before implementation (Phase 3)._

### Public surface

```rust
pub struct AcousticConfig {
    pub driving:    Driving,        // bench knobs (Tier 1)
    pub medium:     MediumSpec,     // physical description (Tier 2)
    pub volume:     VolumeGrid,     // discretization & reproducibility
    pub transducer: TransducerSpec, // excitation boundary
    pub particle:   ParticleSpec,   // Gor'kov object
    pub solver:     SolverParams,   // numerical settings (Tier 3)
}

pub struct Driving {
    pub frequency_hz: f32,  // live-tunable via `set_frequency`
    pub amplitude: f32,     // transducer normal-velocity amplitude u₀ (m/s)
}

pub struct MediumSpec {
    pub density: f32,      // ρ₀ (kg/m³)
    pub sound_speed: f32,  // c (m/s)
}

pub struct VolumeGrid {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    /// Physical span `[ex, ey, ez]` (m). Spacing dx = ex/width, etc.
    pub extent: [f32; 3],
    pub readback_stride: u32,   // per axis; 1 = every node. Fixed at construction.
    pub noise_amplitude: f32,   // initial white-noise pressure seed (Pa)
    pub seed: u64,
}

pub enum Axis { X, Y, Z }
pub enum Side { Low, High }

/// v1: the full face vibrates uniformly (Langevin-type levitator).
pub struct TransducerSpec { pub axis: Axis, pub side: Side }

pub struct ParticleSpec {
    pub radius: f32,       // R (m) — must satisfy R < λ/4 (Gor'kov validity)
    pub density: f32,      // ρ_p (kg/m³)
    pub sound_speed: f32,  // c_p (m/s)
}

pub struct SolverParams {
    pub dt: f32,
    /// EMA window in drive periods (τ = averaging_periods / f).
    pub averaging_periods: f32,
}

pub struct AcousticPressureNode {   // GPU ↔ CPU layout, mirrored + asserted
    pub pressure_pa: f32,
    pub force: [f32; 3],            // F = −∇U (N)
}

impl AcousticSimulation {
    pub fn new(ctx: &GpuContext, config: AcousticConfig) -> Result<Self>;
    pub fn set_frequency(&mut self, frequency_hz: f32);   // no reallocation
    pub fn set_amplitude(&mut self, amplitude: f32);      // no reallocation
    pub fn step(&mut self) -> Result<Vec<AcousticPressureNode>>; // blocking (ADR-0006)
}
```

### Physics model

- Time-domain wave propagation ([ADR-0012](./adr/0012-acoustic-model.md)), first-order form
  `p_t = q`, `q_t = c²∇²p`, semi-implicit Euler at fixed `dt`.
- Rigid walls everywhere (`∂p/∂n = 0` via mirrored ghost cells); the transducer face
  imposes `∂p/∂n ∝ ρ₀·ω·u₀·sin(2π·f·t)` — the standing wave builds up over steps.
- Cycle averages by per-node exponential moving average with τ = `averaging_periods`/f:
  `⟨p²⟩` and `⟨|∇p|²⟩` (velocity recovered as `⟨|v|²⟩ ≈ ⟨|∇p|²⟩/(ρ₀²ω²)`).
- Gor'kov potential `U = V₀[f₁⟨p²⟩/(2ρ₀c²) − 3f₂⟨|∇p|²⟩/(4ρ₀²ω²)]`,
  `V₀ = 4πR³/3`, `f₁ = 1 − ρ₀c²/(ρ_p c_p²)`, `f₂ = 2(ρ_p−ρ₀)/(2ρ_p+ρ₀)`;
  output force `F = −∇U` by central differences on U.
- Initial state: zero pressure + seeded white noise (shared deterministic RNG).

### Preconditions

- **P1** — `width, height, depth ∈ [8, 256]`; all `extent[i] > 0` finite;
  `readback_stride ∈ [1, 64]`; `noise_amplitude ≥ 0`.
- **P2** — floats finite and: `density > 0`, `sound_speed > 0` (medium and particle),
  `radius > 0` with `R < λ/4` where `λ = c/frequency_hz` (Gor'kov validity),
  `amplitude ≥ 0`, `dt > 0`, `averaging_periods > 0`.
- **P3** — `frequency_hz ∈ [20.0, 40_000.0]` Hz.
- **P4** — CFL stability of explicit Euler in 3D:
  `dt · c · sqrt(1/dx² + 1/dy² + 1/dz²) < 1`.

### Postconditions

- **O1** — every `step()` returns exactly
  `⌈width/s⌉·⌈height/s⌉·⌈depth/s⌉` elements (s = stride), row-major with x fastest.
- **O2** — all returned pressures and forces are finite.
- **O3** — `step()` returns only after GPU completion and CPU copy ([ADR-0006](./adr/0006-gpu-cpu-readback-strategy.md)).
- **O4** — same `(config, seed)` ⇒ bit-identical trajectory across runs on the same adapter.

### Invariants

- **I1** — buffers allocated once at construction; only `frequency_hz`/`amplitude`
  are live-mutable (uniform rewrite).
- **I2** — GPU and reference implementations share identical ghost-cell handling,
  stencil weights, forcing phase and update order.
- **I3** — neighbour reads require double buffering: `{p,q}` state ping-pongs between
  two storage buffers via bind-group swap; the Gor'kov pass reads the just-written
  buffer (dispatches ordered within one compute pass).

### Failure modes

- **F1** — any violated precondition → `Error::Acoustic(AcousticError::InvalidConfig)`
  (checked eagerly in `new()`, message names the clause).
- **F2** — readback map/poll failure → `Error::Gpu(..)` ([ADR-0006](./adr/0006-gpu-cpu-readback-strategy.md)).

### Golden-file tolerance (per [ADR-0007](./adr/0007-test-strategy.md))

After 100 steps on the reference config (48³ grid, full-face X/Low transducer),
mean `|Δp|` between GPU (`f32`) output and the CPU (`f64`) reference must stay
below the frozen tolerance.

- **Frozen: `0.25 Pa`** — first measurement on real hardware gave mean
  |Δp| = 1.585e-2 Pa (field scale O(10³) Pa ⇒ ~1e-5 relative, consistent with
  f32 accumulation over a coherently driven build-up).
- The reference drive is **24 kHz**, deliberately detuned off any box
  eigenfrequency (c/2L = 4287.5 Hz; 25 kHz would sit exactly on mode (3,3,4),
  where undamped linear growth amplifies round-off beyond useful bounds).

### Open points

- Circular transducer patch (v1.1) — geometric refinement, no API break expected.
- Absorbing boundaries (impedance BC) for non-reverberant enclosures.


## `GpuContext` (shared)

_Filled from [ADR-0002](./adr/0002-shared-gpu-context.md), [ADR-0003](./adr/0003-wgpu-version-and-backend-strategy.md), [ADR-0005](./adr/0005-central-error-type.md) and [ADR-0006](./adr/0006-gpu-cpu-readback-strategy.md); completed by [ADR-0008](./adr/0008-gpu-only-no-cpu-fallback.md). Written prior to implementation._

### Public surface

- `GpuContext::new() -> Result<GpuContext>` — async construction (adapter/device request is inherently async); the *only* async part of the public API, since `step()` is deliberately synchronous ([ADR-0006](./adr/0006-gpu-cpu-readback-strategy.md)).
- `GpuContext::device() -> &wgpu::Device`, `GpuContext::queue() -> &wgpu::Queue` — how modules build their pipelines/buffers.
- Every fallible Cymatrox API returns `Result<T, cymatrox::Error>` ([ADR-0005](./adr/0005-central-error-type.md)).

### Preconditions

- **P1** — `new()` is called from a context that can await (any async runtime; none is mandated).
- **P2** — the host exposes at least one backend per [ADR-0003](./adr/0003-wgpu-version-and-backend-strategy.md) (Vulkan/Metal/DX12 natively, WebGPU on `wasm32`); a dedicated **or integrated** GPU both count. There is no in-crate CPU fallback ([ADR-0008](./adr/0008-gpu-only-no-cpu-fallback.md)); absence of every backend is an immediate *error*, never a silent degradation.
- **P3** — `new()` takes no configuration; its outcome depends solely on the environment.

### Postconditions

- **O1** — on success, `device()` and `queue()` return handles valid for the whole lifetime of the `GpuContext`.
- **O2** — the device is created with default limits and no extra features; any module needing more must widen this contract explicitly, with justification.
- **O3** — every error carries an actionable message (what failed + likely remedy) — the "never fail silently" rule of [ADR-0003](./adr/0003-wgpu-version-and-backend-strategy.md).
- **O4** — construction never panics because of the environment (headless machine, missing drivers, unsupported browser); every environmental failure maps to an `Err`.

### Invariants

- **I1** — exactly one device and one queue per `GpuContext`; modules obtain them by reference and never create their own ([ADR-0002](./adr/0002-shared-gpu-context.md)).
- **I2** — commands submitted to the queue execute in submission order; interleaved module steps on one context observe FIFO ordering.
- **I3** — `GpuContext` is immutable after construction; nothing reconfigures the device/queue afterwards.
- **I4** — module state crosses back to the CPU only via the staging-buffer pattern of [ADR-0006](./adr/0006-gpu-cpu-readback-strategy.md) (STORAGE → `copy_buffer_to_buffer` → MAP_READ).

### Failure modes

| Variant (indicative names) | Condition | Source |
|---|---|---|
| `Error::Gpu(GpuError::NoAdapter)` | wgpu finds no compatible backend — no GPU at all (neither dedicated nor integrated) | [ADR-0008](./adr/0008-gpu-only-no-cpu-fallback.md) |
| `Error::Gpu(GpuError::Request(_))` | adapter found but device request failed (limits/features) | [ADR-0005](./adr/0005-central-error-type.md) |

Readback map/poll failures happen during module `step()` calls and are specified in each module's contract below; they surface as `Error::Gpu(..)` too ([ADR-0006](./adr/0006-gpu-cpu-readback-strategy.md)).

### Open points

- Whether `GpuContext` additionally guarantees `Send + Sync` on native targets (sharing across threads) — undecided; [ADR-0002](./adr/0002-shared-gpu-context.md) currently mandates sharing by reference only.
