# ADR-0010: Manual WGSL↔Rust type mirroring with assertion tests

**Status:** Accepted (supersedes the `wgsl_bindgen` plan in ARCHITECTURE.md § type generation)

> **In plain terms:** the GPU and CPU struct layouts are written twice — once per side — and a test permanently checks they match. Same guarantee as code generation, minus a fragile build-time dependency.

## Context

ARCHITECTURE.md originally planned to generate Rust structs from WGSL via `wgsl_bindgen` at build time, guaranteeing GPU/CPU layout agreement by construction. In practice, that third-party build dependency moves fast and its compatibility with each wgpu major (currently 30) is not guaranteed for a solo maintainer tracking latest-stable wgpu ([ADR-0003](./0003-wgpu-version-and-backend-strategy.md)).

## Decision

- Structs are defined **once on each side**: WGSL structs live next to their shader; Rust mirrors live in `src/granular/types.rs` (`#[repr(C)]`, `bytemuck::Pod`).
- The sync guarantee is enforced by **assertion tests** (`types.rs::tests::gpu_layouts_match_wgsl_mirror`) checking sizes and field offsets against the documented mirror contract.
- Both files carry header comments stating the pact: *any change must be applied to both sides in the same commit*.
- The goal of ARCHITECTURE.md is unchanged — "the GPU-side and CPU-side struct layouts guaranteed in sync" — only the mechanism differs.

## Alternatives considered

- **Keep `wgsl_bindgen`** — generation removes duplication entirely, but adds a moving build dependency whose breakage lands at the worst time (release bumps); rejected while the project tracks bleeding-edge wgpu.
- **Hand-written types with no assertions** — no enforcement, drift becomes silent corruption; rejected outright.

## Consequences

- Zero build-script dependencies; `cargo build` stays trivially reproducible.
- Adding/altering a shared struct requires touching two files plus one test — cheap, explicit, reviewable.
- If a future release makes codegen viable again, this ADR can be superseded without touching the shader or public API.
