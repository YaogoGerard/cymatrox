//! Async cross-platform readback helper.
//!
//! Uses `futures_channel::oneshot` instead of `std::sync::mpsc` so the
//! channel is safe on WASM (no `Atomics.wait`). On native, `device.poll(Wait)`
//! drives the mapping callback; on WASM the browser's event loop resolves it.

use crate::{Error, GpuError, Result};
use futures_channel::oneshot;

/// Maps the staging buffer asynchronously and reads back the GPU results.
///
/// Works on both native and WASM:
/// - **Native:** `device.poll(Wait)` resolves the mapping callback synchronously.
/// - **WASM:** the browser's event loop resolves it via the `map_async` Promise.
pub(crate) async fn read_back_async<T: bytemuck::Pod + Send>(
    staging: &wgpu::Buffer,
    byte_len: u64,
    #[cfg_attr(target_arch = "wasm32", allow(unused_variables))] device: &wgpu::Device,
) -> Result<Vec<T>> {
    let (tx, rx) = oneshot::channel();
    staging
        .slice(..byte_len)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });

    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
    }

    rx.await
        .map_err(|_| {
            Error::Gpu(GpuError::Readback(
                "mapping callback channel closed unexpectedly".into(),
            ))
        })?
        .map_err(|e| Error::Gpu(GpuError::Readback(e.to_string())))?;

    let out: Vec<T> = {
        let view = staging.slice(..byte_len).get_mapped_range().map_err(|e| {
            Error::Gpu(GpuError::Readback(format!("mapped range unavailable: {e}")))
        })?;
        let bytes: &[u8] = &view;
        bytemuck::cast_slice(bytes).to_vec()
    };
    staging.unmap();

    Ok(out)
}
