# ADR-0001: Single crate, not split per module

**Status:** Accepted

> **In plain terms:** ship one package containing all three physics modules rather than three separate packages — fewer moving parts for a solo maintainer.

## Context

Cymatrox has three physics domains (granular, fluid, acoustic). A common pattern in the Rust ecosystem is to split each into its own crate (e.g. `cymatrox-granular`, `cymatrox-fluid`, `cymatrox-acoustic`) behind a facade, or leave them fully independent.

## Decision

Ship all three modules inside a single `cymatrox` crate, gated by feature flags if compile time becomes an issue later.

## Alternatives considered

- **Per-module crates** — better compile-time isolation and independent versioning, but adds maintenance overhead (three `Cargo.toml`s, three release cycles) for a solo-maintained project, and most users who want cymatics simulation want more than one module.

## Consequences

- Simpler versioning and release process for a single maintainer.
- Users pull in code for modules they may not use, unless/until feature flags are introduced.
- Revisit if the crate grows large enough that compile time or scope becomes a real problem.
