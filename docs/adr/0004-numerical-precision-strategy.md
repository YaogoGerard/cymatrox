# ADR-0004: Numerical precision strategy

**Status:** Accepted

> **In plain terms:** the GPU computes in standard float precision (f32); a slower double-precision (f64) version on CPU serves as the trusted answer key that tests compare against.

## Context

WGSL only natively supports `f32`, so GPU-side computation is constrained to single precision regardless of preference. The open question is whether to build a separate validation path, and in what precision.

## Decision

- All GPU-side compute (WGSL shaders) uses **`f32`** — this is a platform constraint, not a choice.
- Build a **CPU reference implementation in `f64`** for each physics module, used to validate the GPU (`f32`) results and detect numerical drift. This is not the hot path — it's a correctness oracle used in tests, not something end users run for their simulations.
- Comparisons between GPU (`f32`) and CPU reference (`f64`) results use an explicit, documented tolerance per module (exact values to be set per module's `CONTRACT.md` section, since acceptable drift differs between the granular, fluid, and acoustic solvers).

## Alternatives considered

- **No CPU reference, trust the GPU path** — simplest, but leaves no way to catch a broken shader, a wrong constant, or GPU-driver-specific numerical quirks; rejected given this is a scientific tool where correctness is the whole value proposition.
- **CPU reference also in `f32`** — would match GPU precision exactly, but then can't distinguish "expected `f32` rounding" from "actual bug," since both paths would carry the same error class. Using `f64` for the reference gives a precision gap wide enough to trust as ground truth.

## Consequences

- Each module needs two implementations: the WGSL/GPU path (production) and a CPU/`f64` path (test-only, feature-gated e.g. behind a `reference` or `test-utils` feature so it doesn't bloat the release build).
- Test suite needs golden-file or property-based comparisons with per-module tolerances — ties directly into the still-open test strategy question.
- Slightly more implementation work per module, but establishes a real correctness bar instead of "it looks right on screen."
