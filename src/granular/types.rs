// Mirrors `struct Grain` in `granular.wgsl` — keep both in sync.
//
// The layout guarantee required by ARCHITECTURE.md ("single source of truth")
// is enforced here by assertion tests (`tests::gpu_layouts_match_wgsl_mirror`)
// instead of code generation: see docs/adr/0010-manual-type-mirroring.md.

use bytemuck::{Pod, Zeroable};

/// Per-grain physical state crossing the GPU → CPU boundary each `step()`
/// (contract postcondition O1/O2).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct GranularData {
    pub position: [f32; 2],
    pub velocity: [f32; 2],
}

/// One entry of the mode table storage buffer.
///
/// Mirrors `struct ModeEntry` in `granular.wgsl`. Storage-buffer array
/// elements must be 16-byte aligned, hence the explicit pad field.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct ModeEntry {
    pub m: u32,
    pub n: u32,
    /// Drive frequency of this mode, in Hz.
    pub omega_hz: f32,
    pub _pad: u32,
}

/// Per-step uniform block.
///
/// Mirrors `struct Params` in `granular.wgsl`. 8 × f32 = 32 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct GpuParams {
    pub plate_size: f32,
    pub frequency_hz: f32,
    pub amplitude: f32,
    pub dt: f32,
    pub drag: f32,
    pub restitution: f32,
    pub coupling_k: f32,
    /// Simulation time accumulated since construction, in seconds.
    pub time: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    /// These values are the Rust-side half of the layout contract with
    /// `granular.wgsl`. If any fails, update the WGSL structs in the same
    /// commit — never one without the other.
    #[test]
    fn gpu_layouts_match_wgsl_mirror() {
        assert_eq!(size_of::<GranularData>(), 16);
        assert_eq!(offset_of!(GranularData, position), 0);
        assert_eq!(offset_of!(GranularData, velocity), 8);

        assert_eq!(size_of::<ModeEntry>(), 16);
        assert_eq!(offset_of!(ModeEntry, m), 0);
        assert_eq!(offset_of!(ModeEntry, omega_hz), 8);

        assert_eq!(size_of::<GpuParams>(), 32);
        assert_eq!(
            size_of::<GpuParams>() % 16,
            0,
            "uniform blocks are 16-byte multiples"
        );
    }
}
