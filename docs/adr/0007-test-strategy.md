# ADR-0007: Test strategy

**Status:** Accepted

> **In plain terms:** continuous integration runs the *real* shaders on a software GPU (Lavapipe); a result counts as correct when it stays within a per-module tolerance of the f64 answer key.

## Context

Cymatrox needs a way to test WGSL compute shaders in CI (which typically has no GPU), and a way to judge whether a numerical result is "correct" given the `f32` GPU / `f64` CPU reference split from ADR-0004. Both unit-level and full-simulation coverage are needed.

## Decision

**CI execution without hardware GPU:** run the real WGSL/wgpu path in CI using **Lavapipe** (Mesa's software Vulkan implementation) rather than relying solely on the CPU fallback path from ADR-0003. This matters because the CPU fallback is a *different code path* — testing only through it would never exercise the actual shaders that ship to users. Lavapipe runs the genuine WGSL compute pipeline, just without real hardware acceleration.

> **Update:** the in-crate CPU fallback mentioned above was removed by [ADR-0008](./0008-gpu-only-no-cpu-fallback.md). Lavapipe therefore becomes the *only* way to exercise the real shaders without hardware — which strengthens this ADR rather than weakening it.

**Correctness oracle:** golden-file tests comparing GPU (`f32`, via Lavapipe in CI) output against the CPU reference implementation (`f64`, from ADR-0004), using an explicit numerical tolerance **defined per module** in that module's section of `CONTRACT.md` (granular, fluid, and acoustic solvers accumulate error differently and don't share one global epsilon).

**Test levels:**
- **Unit tests** per module — invalid config produces the correct `Error` variant, buffer sizes match expected dimensions, single-step output shape is correct.
- **Integration tests** — a full simulation run over N steps produces output that stays within tolerance of the golden/reference trajectory (catches drift accumulation that a single-step test would miss).

## Alternatives considered

- **CPU-fallback-only testing in CI (no Lavapipe)** — simpler CI setup, but never actually runs the shipped WGSL shaders; a broken shader could pass CI while failing on real GPU hardware. Rejected — defeats the purpose of testing the GPU path at all.
- **Property-based testing instead of golden files** — good for catching edge cases from generated inputs, but harder to reason about "how much drift is acceptable" without a fixed reference trajectory to compare against. Not rejected outright — can be added later as a complement (e.g. fuzzing config inputs for panics/invalid states) but golden-file + tolerance is the primary correctness check, since the domain here is "does the physics match," not "does it crash."
- **A single global tolerance across all modules** — simpler, but physically wrong: granular particle drift, fluid surface tension convergence, and acoustic pressure fields don't have comparable error magnitudes.

## Consequences

- CI pipeline needs Lavapipe/Mesa installed as a dependency — adds setup complexity but is standard practice in the wgpu ecosystem for headless GPU testing.
- Each module's `CONTRACT.md` section must specify its golden-file tolerance as part of its postconditions — this is now a required field, not optional documentation.
- Golden reference data (from the `f64` CPU path) needs to be generated and checked in (or regenerated deterministically) as part of the test fixtures.
- This closes the last open ADR blocking Phase 0 — `GpuContext` and `Error` can now be fully specified in `CONTRACT.md` using ADR-0003 through ADR-0007.
