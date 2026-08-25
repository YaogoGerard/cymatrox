# ADR-0006: GPU→CPU readback strategy

**Status:** Accepted

> **In plain terms:** calling `step()` feels like a normal function call — it waits until the GPU is finished and hands you the numbers. Internally, results hop through a CPU-readable buffer first, because GPUs generally don't allow direct reads of their working memory.

## Context

After each compute pass, module state (`GranularData`, `FluidSurfaceNode`, `AcousticPressureNode`) needs to move from GPU storage buffers to CPU-accessible `Vec<T>`. Two decisions: sync vs async API surface for `step()`, and how the buffer copy itself happens.

## Decision

**`step()` is blocking.** Internally it does `queue.submit(...)`, then `buffer.map_async(...)`, then `device.poll(wgpu::Maintain::Wait)` to force completion, then reads the mapped range into a `Vec<T>` before returning. Callers get a plain `Result<Vec<T>, Error>` — no `Future`, no required async runtime. Cymatrox's target users are scientists running experiments, not necessarily developers comfortable wiring up `tokio`; a synchronous call matches how they'd use MATLAB or NumPy.

**Readback goes through a staging buffer.** Storage buffers (`BufferUsages::STORAGE`) generally cannot be mapped for CPU read on most backends. Each module's compute pass writes to a `STORAGE` buffer, then `copy_buffer_to_buffer` copies it into a dedicated buffer created with `BufferUsages::MAP_READ | BufferUsages::COPY_DST`, which is what actually gets mapped and read. This is a platform constraint from wgpu/WebGPU's buffer usage model, not a stylistic choice.

## Alternatives considered

- **Async `step()` returning a `Future`** — better for embedding cymatrox inside an already-async application (e.g. a web backend), but forces a runtime dependency (tokio, async-std, or wasm-bindgen-futures) onto every user, including the primary "run one experiment, get one result" case. Rejected as the default; may be revisited as an additional `step_async()` if real demand shows up.
- **Direct mapping of storage buffers** — not generally supported; rejected as a non-option, not a preference.

## Consequences

- `step()` blocks the calling thread until the GPU finishes the frame — acceptable for the target usage (offline experiments), but callers embedding cymatrox in a UI loop need to run it off the main thread themselves.
- Each module needs one extra staging buffer alongside its storage buffer, plus the `copy_buffer_to_buffer` step in the compute pass — a fixed, documented pattern all three modules share.
- This blocking + staging-buffer behavior becomes part of `GpuContext`'s and each module's contract: `step()`'s postcondition is "returns only after the GPU has fully completed and results are copied to CPU," and a `Gpu` error variant should cover the case where mapping/polling fails or times out.
