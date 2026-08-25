# ADR-0005: Central error type

**Status:** Accepted

> **In plain terms:** users only need to learn one error type for the whole library; inside, it stays organized by module so details aren't lost.

## Context

Cymatrox has multiple physics modules (granular, fluid, acoustic) plus GPU setup, all of which can fail. The crate needs a consistent error-handling story across module boundaries.

## Decision

A single central `cymatrox::Error` enum, with variants scoped per source, e.g.:

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("GPU initialization failed: {0}")]
    Gpu(GpuError),

    #[error("granular module error: {0}")]
    Granular(GranularError),

    #[error("fluid module error: {0}")]
    Fluid(FluidError),

    #[error("acoustic module error: {0}")]
    Acoustic(AcousticError),
}
```

Each module keeps its own internal error enum (`GranularError`, etc.) for module-local detail, but all public-facing APIs return `cymatrox::Error` (or a `cymatrox::Result<T>` alias), not the module-local type directly. `GpuError` covers the two failure modes from ADR-0003 (no adapter found, no adapter and no CPU fallback available).

> **Update:** since [ADR-0008](./0008-gpu-only-no-cpu-fallback.md) removed the CPU fallback, `GpuError` covers exactly two variants: `NoAdapter` and `Request`.

## Alternatives considered

- **Fully separate per-module error types with no central enum** — more idiomatic if modules were separate crates, but forces callers using more than one module to match on multiple unrelated error types; inconsistent with ADR-0001's single-crate decision.
- **A single flat enum with no per-module nesting** — simpler on paper, but loses structure once each module's error detail grows (e.g. distinguishing a granular config error from a granular numerical instability error).

## Consequences

- One `Result<T, cymatrox::Error>` alias used consistently across the public API — this becomes part of `GpuContext`'s and each module's contract (`CONTRACT.md`) as the standard failure signature.
- Module-internal error enums stay private implementation detail unless there's a reason to expose them; `cymatrox::Error` is the only error type users need to learn.
- Uses `thiserror` for boilerplate-free `Display`/`Error` impls (standard in the Rust ecosystem for library error types, as opposed to `anyhow` which is for application code).
