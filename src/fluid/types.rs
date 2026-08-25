//! GPU ↔ CPU shared layouts of the fluid module (ADR-0010 pact).
//!
//! Every struct here has a hand-written WGSL twin in `fluid.wgsl`. The
//! assertion tests below enforce that the Rust side keeps the exact size
//! the shader expects. Any change must touch both sides in the same commit.

/// One surface mesh point — the module's public readback element.
///
/// WGSL twin:
/// ```wgsl
/// struct FluidSurfaceNode { height: f32, velocity_y: f32 } // 8 bytes
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct FluidSurfaceNode {
    /// Surface elevation η (m).
    pub height: f32,
    /// Vertical velocity ∂η/∂t (m/s).
    pub velocity_y: f32,
}

/// Per-step uniform block.
///
/// WGSL twin (`Params` in `fluid.wgsl`) — 16 × 4-byte scalars = 64 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct GpuParams {
    /// Grid width in nodes.
    pub grid_w: u32,
    /// Grid height in nodes.
    pub grid_h: u32,
    /// Output width after striding (nodes).
    pub out_w: u32,
    /// Stride along each axis (≥ 1).
    pub stride: u32,
    /// Grid spacing dx = extent_w / width (m).
    pub dx: f32,
    /// Grid spacing dy (m).
    pub dy: f32,
    /// Base gravity coefficient g·h (m³/s²).
    pub gh_base: f32,
    /// Capillary coefficient σ·h/ρ (m³/s²).
    pub sigma_h_rho: f32,
    /// Phenomenological damping γ (1/s).
    pub damping_gamma: f32,
    /// Time step dt (s).
    pub dt: f32,
    /// Drive angular frequency 2π·f (rad/s).
    pub drive_omega: f32,
    /// Drive acceleration × depth, a·h (m³/s²) — the Mathieu modulation depth.
    pub drive_accel_h: f32,
    /// Simulation time at step start (s).
    pub time: f32,
    /// Squared mask radius; negative disables the circular mask (`Full`).
    pub radius_sq: f32,
    /// Centre x of the buffer (extent_w / 2) (m).
    pub centre_x: f32,
    /// Centre y of the buffer (extent_h / 2) (m).
    pub centre_y: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    const NODE_WGSL_BYTES: usize = 8; // 2 × f32
    const PARAMS_WGSL_BYTES: usize = 64; // 16 × f32-like scalars

    #[test]
    fn gpu_layouts_match_wgsl_mirror() {
        assert_eq!(
            std::mem::size_of::<FluidSurfaceNode>(),
            NODE_WGSL_BYTES,
            "FluidSurfaceNode drifted from its WGSL twin — update both sides \
             in the same commit (ADR-0010)"
        );
        assert_eq!(
            std::mem::size_of::<GpuParams>(),
            PARAMS_WGSL_BYTES,
            "GpuParams drifted from the WGSL `Params` block — update both sides \
             in the same commit (ADR-0010)"
        );
        assert_eq!(
            std::mem::offset_of!(FluidSurfaceNode, velocity_y),
            4,
            "velocity_y must sit at offset 4"
        );
    }

    #[test]
    fn pod_zeroable() {
        let n = FluidSurfaceNode {
            height: 1.5,
            velocity_y: -0.25,
        };
        let bytes: &[u8] = bytemuck::bytes_of(&n);
        assert_eq!(bytes.len(), 8);
        assert!(bytemuck::try_from_bytes::<FluidSurfaceNode>(bytes).is_ok());
    }
}
