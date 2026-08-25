# ADR-0003: wgpu version and backend strategy

**Status:** Accepted — the CPU-fallback clause is superseded by [ADR-0008](./0008-gpu-only-no-cpu-fallback.md) (GPU strictly required)

> **In plain terms:** build against a recent wgpu and support every major platform (desktop and web); if no GPU exists, stop with a clear, honest error instead of degrading silently. *(The CPU-fallback clause originally recorded here was superseded by [ADR-0008](./0008-gpu-only-no-cpu-fallback.md).)*

## Context

Cymatrox needs a concrete decision on which wgpu version to depend on, which GPU backends to target, and what happens when no GPU backend is available. This affects `GpuContext` initialization directly and must be settled before its contract (preconditions/failure modes) can be written.

## Decision

- Pin to the **latest stable wgpu** release at time of each Cymatrox release (not a fixed old version) — bumped deliberately per release, not left to float via semver-compatible auto-updates.
- Target **all backends wgpu exposes on native platforms** (Vulkan, Metal, DX12) via its default backend selection, **plus WebGPU** for browser targets (via `wasm32-unknown-unknown` + `wgpu`'s WebGPU backend).
- **Fallback order:** try to acquire a GPU adapter through wgpu's normal backend selection first. If none is available, attempt a **CPU-based fallback path** (e.g. `wgpu`'s software adapter where supported, or a dedicated CPU compute path if wgpu offers none). If CPU fallback is also not possible, **cancel initialization and return a clear, actionable error** — never fail silently or fall back to a degraded/partial simulation without telling the caller.

## Alternatives considered

- **Pin to a fixed wgpu version** — more reproducible builds, but forgoes backend/driver fixes and new features; rejected in favor of deliberate version bumps per release instead of a stale pin.
- **Native-only, no WebGPU** — simpler test matrix, but cuts off browser-based use cases (e.g. running experiments from a web notebook), which is a real target use case for scientists.
- **No CPU fallback, GPU required** — simpler `GpuContext`, but breaks on CI runners and headless machines without a GPU; rejected since dev/CI usability matters for an open-source crate expecting contributions.

## Consequences

- `GpuContext::new()` becomes fallible with a distinct error path for "no GPU backend" vs "no GPU and CPU fallback unavailable" — this is a required failure mode in `CONTRACT.md`.
- CPU fallback path needs its own (likely much slower) implementation or a software adapter dependency — scoped separately, not assumed free.
- Browser builds require testing the `wasm32-unknown-unknown` target explicitly, not just native.
