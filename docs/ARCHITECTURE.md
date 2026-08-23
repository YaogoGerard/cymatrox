# Architecture

## Overview

Cymatrox is a single crate exposing three independent physics modules that share one GPU context. It has no rendering or UI layer — its only job is to compute physical state and hand it back as plain data.

```
cymatrox
├── core/        GpuContext, shared error type, buffer/bindgen utilities
├── granular/    Module 1 — solid (Chladni plates)
├── fluid/       Module 2 — liquid surface (CymaScope)
├── acoustic/    Module 3 — gas (acoustic levitation)
└── io/          Audio input (cpal + FFT), CSV/JSON/binary export
```

## Shared GPU context

A single `GpuContext` (device + queue) is created once and passed by reference into every module. Modules do not create their own `wgpu::Device` — this avoids redundant GPU initialization and lets multiple simulations run side by side. See [ADR-0002](./adr/0002-shared-gpu-context.md).

## Module data flow

Each module follows the same shape:

1. **Config in** — plate/grid/volume dimensions, physical constants, initial conditions.
2. **Compute step** — a WGSL compute shader executes the module's physics equation on the GPU.
3. **Data out** — a `Vec<T>` of per-element state (position, height, or pressure), read back via `bytemuck` for zero-copy CPU/GPU transfer.

| Module | Equation | Per-step output |
|---|---|---|
| Granular | Sophie Germain plate equation + Newtonian kinetics (friction, rebound) | `Vec<GranularData>` |
| Fluid | Mathieu equation (Faraday instability) + Navier-Stokes + Laplace-Young surface tension | `Vec<FluidSurfaceNode>` |
| Acoustic | Helmholtz equation + Gor'kov force potential | `Vec<AcousticPressureNode>` |

## WGSL ↔ Rust type generation

Struct definitions live once in WGSL and are generated into Rust via `wgsl_bindgen` as a build dependency (`build.rs` → `OUT_DIR` → `include!`). This keeps the GPU-side and CPU-side struct layouts guaranteed in sync — no hand-maintained duplicate types.

## Why a single crate

Cymatrox is one crate rather than split per module (e.g. no `cymatrox-granular`, `cymatrox-fluid`). See [ADR-0001](./adr/0001-single-crate-architecture.md) for the reasoning and trade-offs.

## Contracts and invariants

Per-module preconditions, postconditions, and invariants (config validity, numerical stability bounds, buffer size guarantees) are documented in [`CONTRACT.md`](./CONTRACT.md).
