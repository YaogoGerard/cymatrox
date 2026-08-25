# ADR-0002: Shared `GpuContext` instance

**Status:** Accepted

> **In plain terms:** all simulations share a single connection to the GPU — created once and passed around — instead of each opening its own.

## Context

Each module (granular, fluid, acoustic) needs a `wgpu::Device` and `wgpu::Queue` to run compute shaders. These could be created once per module, or shared.

## Decision

Provide a single `GpuContext`, created once, passed by reference into every module's constructor.

## Alternatives considered

- **Per-module device/queue** — simpler module isolation, but wastes GPU initialization cost and prevents running multiple modules concurrently against the same GPU resources.

## Consequences

- Modules cannot be constructed without an existing `GpuContext` — slightly more setup code for the caller, documented in the quickstart.
- Multiple simulations (e.g. granular + fluid side by side) share one GPU context efficiently.
