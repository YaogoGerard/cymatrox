# Glossary

Every technical term used in this repository, explained in plain language. Terms are grouped by theme — no prior knowledge assumed.

## Physics & sound

- **Cymatics** — the study of *visible* sound: what patterns appear when a surface, a liquid, or particles vibrate at specific frequencies. Classic demonstration: sand on a vibrating plate arranging itself into geometric figures.
- **Chladni plate** — a metal plate dusted with sand. Vibrated at the right frequency, the sand gathers along lines where the plate barely moves, forming geometric patterns.
- **Nodal line** — a region of a vibrating surface that stays almost still. Particles naturally drift toward these calm zones.
- **Sophie Germain plate equation** — the mathematics describing how a thin elastic plate bends under vibration. Predicts the shapes of the patterns on a Chladni plate.
- **Faraday instability (Faraday waves)** — regular ripples that suddenly appear on a liquid's surface when it is shaken vertically harder than a threshold.
- **Mathieu equation** — a classic equation for systems whose properties oscillate in time (here: the vertically shaken liquid). Predicts when the Faraday ripples appear.
- **Navier–Stokes equations** — the fundamental equations of fluid motion: how a liquid's velocity and pressure evolve over time.
- **Laplace–Young law** — relates the curvature of a liquid surface to the pressure difference across it; the physics behind surface-tension effects like droplets beading up.
- **Acoustic levitation** — holding small particles or droplets in mid-air using nothing but the pressure of standing sound waves.
- **Helmholtz equation** — describes the spatial shape of a sound-pressure field at a given frequency.
- **Gor'kov potential** — a formula giving the net acoustic force on a small object inside a sound field; its minima tell you where the object will get trapped (the levitation points).

## GPU & computing

- **CPU vs GPU** — the CPU has a few very capable cores; the GPU has thousands of small ones. When millions of things need the same kind of computation, the GPU wins by a large margin — hence Cymatrox's design.
- **Adapter** — a concrete GPU the program can use: your graphics card, or a software implementation of one.
- **Backend** — the low-level API used to talk to the GPU: Vulkan, Metal, DX12, or WebGPU. See [ADR-0003](./adr/0003-wgpu-version-and-backend-strategy.md).
- **wgpu** — a Rust library implementing the WebGPU API on top of all native backends. It is how Cymatrox talks to the GPU.
- **WGSL** — WebGPU Shading Language, the language compute programs are written in. Cymatrox's physics steps are WGSL shaders.
- **Compute shader** — a program the GPU executes over huge amounts of data in parallel. Each Cymatrox module runs one per time step.
- **Device & queue** — the two handles a program holds onto the GPU. The *device* creates resources (buffers, pipelines); the *queue* receives submitted work and executes it strictly in order.
- **Storage buffer** — GPU memory a compute shader reads and writes. On most platforms it cannot be read directly by the CPU.
- **Staging buffer** — a small CPU-readable buffer. Results are copied into it before being read back. See [ADR-0006](./adr/0006-gpu-cpu-readback-strategy.md).
- **Mapping / readback** — making GPU memory visible to CPU code and copying it into a `Vec`.
- **f32 / f64** — 32-bit vs 64-bit floating-point numbers. GPUs natively compute in f32 (fast, ~7 significant digits); CPUs comfortably handle f64 (~16 digits). See [ADR-0004](./adr/0004-numerical-precision-strategy.md).

## Testing & quality

- **Reference implementation** — a slower CPU version of the same physics, computed in f64. It acts as the trusted "answer key" for tests.
- **Golden-file test** — a test that compares program output against stored, known-good results.
- **Tolerance** — how far GPU (f32) results may deviate from the f64 reference before being considered wrong. Set per module, since each solver accumulates error differently. See [ADR-0007](./adr/0007-test-strategy.md).
- **Lavapipe** — a software implementation of Vulkan (from Mesa): a "fake GPU" running on the CPU. It lets continuous integration execute the *real* shaders on machines without graphics hardware.
- **Feature flag** — a compile-time switch (e.g. `reference`) that keeps test-only code out of release builds.

## Project & process

- **ADR** — Architecture Decision Record: a short document capturing one design decision, the alternatives considered, and the consequences. All of Cymatrox's live in [`docs/adr/`](./adr/).
- **Contract & invariants** — the promises a piece of the library makes: what must be true before a call (preconditions), what is guaranteed after (postconditions), and what always holds (invariants). Written down *before* code in [`CONTRACT.md`](./CONTRACT.md).
