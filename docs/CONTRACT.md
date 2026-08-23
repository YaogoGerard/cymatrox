# Contract & Invariants

This document lists the preconditions, postconditions, and invariants each module guarantees. It's part of the "Contract & Invariants before code" step of the design process, and is meant to be filled in per module as it's implemented — not written from the API after the fact.

## How to fill this in, per module

For each public function/struct, specify:

- **Preconditions** — what must be true of the input for the call to be valid (e.g. grid dimensions > 0, frequency within a physically meaningful range).
- **Postconditions** — what's guaranteed true of the output on success (e.g. output `Vec` length always equals grid resolution).
- **Invariants** — what stays true across the object's lifetime (e.g. `GpuContext` device handle never becomes invalid while the struct lives).
- **Failure modes** — what causes an `Err` variant, and which one.

## Granular module

_To be filled in during Phase 1._

## Fluid module

_To be filled in during Phase 2._

## Acoustic module

_To be filled in during Phase 3._

## `GpuContext` (shared)

_To be filled in during Phase 0._
