use crate::{GpuError, Result};

/// Shared GPU context: one device + one queue for every module (ADR-0002).
///
/// Guarantees: see `docs/CONTRACT.md` § `GpuContext`.
pub struct GpuContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl GpuContext {
    /// Creates the shared context.
    ///
    /// The only async part of the public API (ADR-0006): `step()` stays sync,
    /// but adapter/device acquisition is inherently asynchronous.
    ///
    /// Contract: P3 (no configuration), O4 (never panics on a bad
    /// environment — everything maps to `Err`).
    pub async fn new() -> Result<Self> {
        let instance = wgpu::Instance::default();

        // Contract P2/F1: ask wgpu for any compatible backend
        // (Vulkan/Metal/DX12/WebGPU — integrated GPUs count).
        //
        // HighPerformance favors the discrete GPU on dual-GPU laptops;
        // an iGPU alone remains a perfectly valid pick.
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
                // Native scientific use: no browser-style fingerprinting
                // constraints, keep the adapter's full default limits.
                apply_limit_buckets: false,
            })
            .await
            .map_err(|_| GpuError::NoAdapter)?;

        // Contract O2: default limits, zero extra features.
        // Any future module needing more must widen this contract first.
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("cymatrox.gpu_context"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            })
            .await
            .map_err(GpuError::from)?;

        Ok(Self { device, queue })
    }

    /// Device handle, valid for the whole lifetime of the context (O1).
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// Queue handle, valid for the whole lifetime of the context (O1).
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs on developer machines and in CI (under Lavapipe, ADR-0007).
    #[tokio::test]
    #[ignore = "requires a GPU-capable host (or software backend)"]
    async fn creates_context_and_exposes_handles() {
        let ctx = GpuContext::new().await.expect("context creation failed");
        // Touch both accessors so they are exercised (contract O1).
        let _ = std::hint::black_box((ctx.device(), ctx.queue()));
    }
}
