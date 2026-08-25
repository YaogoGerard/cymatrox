//! GPU ↔ CPU shared layouts of the acoustic module (ADR-0010 pact).
//!
//! Every struct here has a hand-written WGSL twin in `acoustic.wgsl`. The
//! assertion tests below enforce that the Rust side keeps the exact size
//! the shader expects. Any change must touch both sides in the same commit.

/// One volume cell — the module's public readback element.
///
/// WGSL twin uses `array<f32, 3>` (not `vec3<f32>`) for the force so the
/// struct stays 16 bytes: `vec3` would impose 16-byte alignment and grow
/// the struct to 32.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct AcousticPressureNode {
    /// Sound pressure p (Pa).
    pub pressure_pa: f32,
    /// Gor'kov radiation force F = −∇U (N).
    pub force: [f32; 3],
}

/// Per-step uniform block.
///
/// WGSL twin (`Params` in `acoustic.wgsl`) — 24 × 4-byte scalars = 96 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct GpuParams {
    pub grid_x: u32,
    pub grid_y: u32,
    pub grid_z: u32,
    /// Strided output width (nodes).
    pub out_x: u32,
    /// Strided output height (nodes).
    pub out_y: u32,
    /// Stride along each axis (≥ 1).
    pub stride: u32,
    /// Transducer normal axis: 0=X, 1=Y, 2=Z.
    pub axis: i32,
    /// Transducer side: 0=Low, 1=High.
    pub side: i32,
    pub dx: f32,
    pub dy: f32,
    pub dz: f32,
    /// Squared sound speed c² (m²/s²).
    pub c2: f32,
    /// Medium density ρ₀ (kg/m³).
    pub rho0: f32,
    pub dt: f32,
    /// Drive angular frequency 2π·f (rad/s).
    pub omega: f32,
    /// Transducer normal-velocity amplitude u₀ (m/s).
    pub drive_u: f32,
    /// Simulation time at step start (s).
    pub time: f32,
    /// EMA weight of the newest sample, α = 1 − exp(−dt/τ).
    pub ema_alpha: f32,
    /// Neumann forcing amplitude ρ₀·ω·u₀ (Pa/m).
    pub neumann_amp: f32,
    pub _pad0: f32,
    /// Gor'kov coefficient V₀·f₁/(2ρ₀c₀²).
    pub gk_p_coeff: f32,
    /// Gor'kov coefficient −V₀·3f₂/(4ρ₀²ω²).
    pub gk_g_coeff: f32,
    pub _pad1: f32,
    pub _pad2: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    const NODE_WGSL_BYTES: usize = 16; // f32 + [f32; 3]
    const PARAMS_WGSL_BYTES: usize = 96; // 24 × scalar

    #[test]
    fn gpu_layouts_match_wgsl_mirror() {
        assert_eq!(
            std::mem::size_of::<AcousticPressureNode>(),
            NODE_WGSL_BYTES,
            "AcousticPressureNode drifted from its WGSL twin — update both sides \
             in the same commit (ADR-0010)"
        );
        assert_eq!(
            std::mem::size_of::<GpuParams>(),
            PARAMS_WGSL_BYTES,
            "GpuParams drifted from the WGSL `Params` block — update both sides \
             in the same commit (ADR-0010)"
        );
        assert_eq!(std::mem::offset_of!(AcousticPressureNode, force), 4);
        assert_eq!(std::mem::offset_of!(GpuParams, _pad0), 76);
        assert_eq!(std::mem::offset_of!(GpuParams, _pad2), 92);
    }

    #[test]
    fn pod_zeroable() {
        let n = AcousticPressureNode {
            pressure_pa: 12.5,
            force: [1.0, -2.0, 0.5],
        };
        let bytes: &[u8] = bytemuck::bytes_of(&n);
        assert_eq!(bytes.len(), 16);
        assert!(bytemuck::try_from_bytes::<AcousticPressureNode>(bytes).is_ok());
    }
}
