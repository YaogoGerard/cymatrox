# Contributing

Thanks for your interest in Cymatrox! The project is currently at the **design/documentation stage** — there is no implementation yet.

## Current focus

- Refining [`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md), the ADRs in [`docs/adr/`](./docs/adr/), and [`docs/CONTRACT.md`](./docs/CONTRACT.md).
- Cymatrox follows a **contracts-before-code** process: each module's preconditions, postconditions, invariants, and failure modes are written in `CONTRACT.md` *before* any implementation lands.

## How to propose changes

1. Open an issue first describing the problem or idea.
2. For architectural decisions, add a new ADR using [`docs/adr/template.md`](./docs/adr/template.md).
3. Documentation PRs must keep the repo self-consistent: every relative link resolves, and the README reflects reality.

## Once code starts landing

Standard Rust workflow: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`. CI runs the same checks plus the GPU golden-file tests on Lavapipe (software Vulkan) — see `.github/workflows/ci.yml` and [ADR-0007](./docs/adr/0007-test-strategy.md). Conventional commits preferred.

## License

Dual-licensed under MIT or Apache-2.0 (`SPDX-License-Identifier: MIT OR Apache-2.0`). Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work is licensed under both, without additional terms — mirroring the Apache-2.0 §5 default.
