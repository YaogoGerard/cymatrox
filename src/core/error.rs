use crate::acoustic::AcousticError;
use crate::fluid::FluidError;
use crate::granular::GranularError;
use thiserror::Error;

/// Top-level error type returned by every fallible Cymatrox API (ADR-0005).
#[derive(Debug, Error)]
pub enum Error {
    #[error("GPU initialization failed: {0}")]
    Gpu(#[from] GpuError),

    #[error("granular module error: {0}")]
    Granular(#[from] GranularError),

    #[error("fluid module error: {0}")]
    Fluid(#[from] FluidError),

    #[error("acoustic module error: {0}")]
    Acoustic(#[from] AcousticError),
}

/// GPU-side failures (`GpuContext` contract, failure modes F1–F2).
#[derive(Debug, Error)]
pub enum GpuError {
    /// Contract P2/F1: no compatible backend found (Vulkan/Metal/DX12/WebGPU).
    #[error(
        "no compatible GPU backend found (tried Vulkan/Metal/DX12/WebGPU); \
         check your drivers or install a software fallback"
    )]
    NoAdapter,

    /// Contract F2: adapter found but device creation failed.
    #[error("GPU device request failed: {0}")]
    Request(#[from] wgpu::RequestDeviceError),

    /// Module readback failure: buffer mapping or device poll did not
    /// complete (`GpuContext` contract F2, ADR-0006).
    #[error("GPU readback failed while retrieving simulation results: {0}")]
    Readback(String),
}

/// Standard alias used by every public API (ADR-0005).
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    /// Contract O3: errors must be actionable.
    #[test]
    fn no_adapter_message_is_actionable() {
        let msg = Error::Gpu(GpuError::NoAdapter).to_string();
        assert!(msg.contains("Vulkan"), "message should hint at backends");
        assert!(msg.contains("drivers"), "message should suggest a remedy");
    }

    #[test]
    fn gpu_error_converts_into_top_level_error() {
        let e: Error = GpuError::NoAdapter.into();
        assert!(matches!(e, Error::Gpu(_)));
    }
}
