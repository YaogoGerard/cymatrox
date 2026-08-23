# Cymatrox

> GPU-accelerated scientific toolkit for cymatics simulation — solid, liquid, and gas — written in Rust.

[![Crates.io](https://img.shields.io/crates/v/cymatrox.svg)](https://crates.io/crates/cymatrox)
[![docs.rs](https://img.shields.io/docsrs/cymatrox)](https://docs.rs/cymatrox)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](#license)
[![Build Status](https://img.shields.io/github/actions/workflow/status/YaogoGerard/cymatrox/ci.yml)](https://github.com/YaogoGerard/cymatrox/actions)

> **Status:** pre-release, not yet published to crates.io. API may change.

## Why Cymatrox

Cymatics — the study of visible sound and vibration — is a well-established field, but no Rust crate exists to run cymatics experiments or build simulators from a computer. Cymatrox fills that gap as a **headless, physics-first library**: it computes, you visualize (in whatever engine or tool you prefer).

## Features

Cymatrox ships three independent physics modules, all running on the GPU via [wgpu](https://github.com/gfx-rs/wgpu):

| Module | Domain | Physics | Output |
|---|---|---|---|
| **Granular** | Solid (Chladni plates) | Sophie Germain plate equation + Newtonian particle kinetics | `Vec<GranularData>` (position + velocity per grain) |
| **Fluid** | Liquid surface (CymaScope) | Mathieu equation (Faraday instability) + incompressible Navier-Stokes + Laplace-Young surface tension | `Vec<FluidSurfaceNode>` (height + vertical velocity per mesh point) |
| **Acoustic** | Gas (acoustic levitation) | Helmholtz equation + Gor'kov force potential | `Vec<AcousticPressureNode>` (pressure in Pa + force vector) |

- Pure computation, no UI — export results as CSV, JSON, or binary for use in MATLAB, Python/NumPy, R, or a rendering engine like Bevy.
- Shared `GpuContext` across modules — no redundant device/queue setup.
- Optional audio-driven input (microphone or file) via `cpal` + FFT.

## Installation

Not yet on crates.io. Until the first release, depend on the Git repository:

```toml
[dependencies]
cymatrox = { git = "https://github.com/YaogoGerard/cymatrox" }
```

## Quickstart

```rust
use cymatrox::{GpuContext, granular::GranularSimulation};

#[tokio::main]
async fn main() -> Result<(), cymatrox::Error> {
    let ctx = GpuContext::new().await?;
    let mut sim = GranularSimulation::new(&ctx, /* config */)?;

    sim.set_frequency(432.0);
    let frame: Vec<_> = sim.step()?;

    // export, plot, or feed into your own renderer
    println!("{} grains simulated", frame.len());
    Ok(())
}
```

See [`examples/`](./examples) for full runnable programs per module.

## Architecture

See [`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md) for the data flow, module boundaries, and shared `GpuContext` design. Design decisions are recorded as ADRs in [`docs/adr/`](./docs/adr).

## Performance targets

| Module | Target scale | Frame rate |
|---|---|---|
| Granular | 100k – 1M particles | ≥ 60 FPS |
| Fluid | 512×512 – 2048×2048 grid | ≥ 60 FPS |
| Acoustic | 64³ – 256³ volume | ≥ 60 FPS |

## Roadmap

- [x] Phase 0 — Foundations (`GpuContext`, error types, build pipeline)
- [ ] Phase 1 — Granular module
- [ ] Phase 2 — Fluid module
- [ ] Phase 3 — Acoustic module
- [ ] Phase 4 — Integration, polish, `v0.1.0` release

## Contributing

Contributions are welcome — see [`CONTRIBUTING.md`](./CONTRIBUTING.md) for setup and conventions.

## License

Dual-licensed under [MIT](./LICENSE-MIT) or [Apache-2.0](./LICENSE-APACHE), at your option.
