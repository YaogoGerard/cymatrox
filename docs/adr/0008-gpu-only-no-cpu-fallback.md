# ADR-0008: GPU-only execution, no CPU fallback

**Status:** Accepted (supersedes the CPU-fallback clause of [ADR-0003](./0003-wgpu-version-and-backend-strategy.md))

> **In plain terms:** Cymatrox needs at least one GPU on the machine — dedicated **or integrated**. If none exists, initialization stops with a clear error; there is no in-crate CPU compute path.

## Context

[ADR-0003](./0003-wgpu-version-and-backend-strategy.md) originally planned a CPU-based fallback path for machines without any usable GPU backend, to be scoped separately later. Maintaining two complete compute implementations per physics module doubles the work for a solo maintainer, and such a fallback would never be exercised by the primary use case: scientists running experiments on real hardware. The decision was revisited before Phase 0 landed any module.

## Decision

- **At least one GPU adapter is required** — a dedicated card or an integrated GPU both count; wgpu exposes either as an ordinary adapter.
- If no adapter is found, `GpuContext::new()` fails immediately with an actionable error (`Error::Gpu(GpuError::NoAdapter)`). No silent degradation, ever.
- There is **no in-crate CPU compute path**, and none will be added unless a new ADR reverses this.
- This is distinct from *system-level* software backends (e.g. Mesa's Lavapipe): installing one exposes a real Vulkan adapter that runs genuine WGSL shaders through wgpu. That is an environment option for CI ([ADR-0007](./0007-test-strategy.md)), not a Cymatrox code path.

## Alternatives considered

- **Keep the CPU fallback from ADR-0003** — preserves headless/no-GPU support, but doubles the implementation and testing surface of every module for a scenario target users rarely hit; rejected.
- **Degrade silently to a slow mode** — violates the "never fail silently" rule of ADR-0003; rejected outright.

## Consequences

- `GpuError::NoFallbackAvailable` is removed; the contract's failure table shrinks to two variants (`NoAdapter`, `Request`) — see [CONTRACT.md](../CONTRACT.md).
- Machines without any GPU backend cannot run Cymatrox; the error message points at driver checks or installing a software backend.
- Per-module effort stays focused on a single WGSL implementation each. The f64 CPU *reference* implementations of [ADR-0004](./0004-numerical-precision-strategy.md) are unaffected: they are a test-only correctness oracle, not a runtime path.
