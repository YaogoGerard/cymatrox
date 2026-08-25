//! Thin wasm-bindgen bridge over the real, published `cymatrox` crate.
//!
//! This file adds no physics — every simulation call below goes straight
//! into `cymatrox::{granular,fluid,acoustic}`. Its only job is JS <-> Rust
//! marshalling: primitive args in, a flat `Float32Array` of the module's
//! real output struct out.

use std::cell::RefCell;
use wasm_bindgen::prelude::*;
use cymatrox::{GpuContext};
use cymatrox::granular::{
    Driving as GranularDriving, GrainBed, GranularConfig, GranularSimulation,
    InitialDistribution, ModeSelection, PlateSpec, SolverParams as GranularSolver,
};
use cymatrox::fluid::{
    Driving as FluidDriving, DomainMask, DomainShape, FluidConfig, FluidSimulation, LiquidSpec,
    SolverParams as FluidSolver, SurfaceGrid,
};
use cymatrox::acoustic::{
    AcousticConfig, AcousticSimulation, Axis, Driving as AcousticDriving, MediumSpec,
    ParticleSpec, Side, SolverParams as AcousticSolver, TransducerSpec, VolumeGrid,
};

thread_local! {
    static GPU: RefCell<Option<GpuContext>> = RefCell::new(None);
    static GRANULAR: RefCell<Option<GranularSimulation>> = RefCell::new(None);
    static FLUID: RefCell<Option<FluidSimulation>> = RefCell::new(None);
    static ACOUSTIC: RefCell<Option<AcousticSimulation>> = RefCell::new(None);
}

fn js_err(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

fn with_gpu<R>(f: impl FnOnce(&GpuContext) -> Result<R, JsValue>) -> Result<R, JsValue> {
    GPU.with(|g| {
        let borrow = g.borrow();
        let ctx = borrow
            .as_ref()
            .ok_or_else(|| JsValue::from_str("call init_gpu() first"))?;
        f(ctx)
    })
}

/// Must be called once, before any `*_start` function. The only async
/// call in the whole API (mirrors ADR-0006: adapter/device acquisition
/// is inherently async, `step()` stays synchronous).
#[wasm_bindgen]
pub async fn init_gpu() -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    let ctx = GpuContext::new().await.map_err(js_err)?;
    GPU.with(|g| *g.borrow_mut() = Some(ctx));
    Ok(())
}

// ---------------------------------------------------------------------
// Granular
// ---------------------------------------------------------------------

/// Creates (or replaces) the granular simulation.
/// `mode_m`/`mode_n` select an explicit Chladni mode pair, matching the
/// (n, m) sliders in the site's UI.
#[wasm_bindgen]
pub fn granular_start(
    count: u32,
    side: f32,
    frequency_hz: f32,
    mode_m: u32,
    mode_n: u32,
    seed: u64,
) -> Result<(), JsValue> {
    with_gpu(|ctx| {
        let config = GranularConfig {
            experiment: GranularDriving {
                frequency_hz,
                amplitude: 1e-4,
                modes: ModeSelection::Explicit(vec![(mode_m, mode_n)]),
            },
            medium: PlateSpec::Idealized { side },
            grains: GrainBed {
                count,
                distribution: InitialDistribution::Uniform,
                seed,
            },
            solver: GranularSolver {
                dt: 1.0 / 480.0,
                drag: 4.0,
                restitution: 0.6,
                coupling_k: 5.0e5,
                base_frequency_hz: 120.0,
            },
        };
        let sim = GranularSimulation::new(ctx, config).map_err(js_err)?;
        GRANULAR.with(|s| *s.borrow_mut() = Some(sim));
        Ok(())
    })
}

/// Advances one step and returns `[x0, y0, x1, y1, ...]` (positions only).
#[wasm_bindgen]
pub fn granular_step() -> Result<Vec<f32>, JsValue> {
    GRANULAR.with(|s| {
        let mut borrow = s.borrow_mut();
        let sim = borrow
            .as_mut()
            .ok_or_else(|| JsValue::from_str("call granular_start() first"))?;
        let frame = sim.step().map_err(js_err)?;
        Ok(frame.iter().flat_map(|g| g.position).collect())
    })
}

#[wasm_bindgen]
pub fn granular_set_frequency(hz: f32) -> Result<(), JsValue> {
    GRANULAR.with(|s| {
        let mut b = s.borrow_mut();
        let sim = b.as_mut().ok_or_else(|| JsValue::from_str("not started"))?;
        sim.set_frequency(hz);
        Ok(())
    })
}

// ---------------------------------------------------------------------
// Fluid
// ---------------------------------------------------------------------

#[wasm_bindgen]
pub fn fluid_start(
    width: u32,
    height: u32,
    extent_x: f32,
    extent_y: f32,
    frequency_hz: f32,
    circular: bool,
    radius: f32,
    seed: u64,
) -> Result<(), JsValue> {
    with_gpu(|ctx| {
        let config = FluidConfig {
            driving: FluidDriving {
                frequency_hz,
                amplitude: 2.0,
            },
            liquid: LiquidSpec {
                density: 1000.0,
                surface_tension: 0.072,
                depth: 0.01,
                damping: 0.5,
                gravity: 9.81,
            },
            surface: SurfaceGrid {
                width,
                height,
                extent: [extent_x, extent_y],
                readback_stride: 1,
                noise_amplitude: 1e-6,
                seed,
            },
            domain: DomainMask {
                shape: if circular {
                    DomainShape::Circular { radius }
                } else {
                    DomainShape::Full
                },
            },
            solver: FluidSolver { dt: 1.0 / 960.0 },
        };
        let sim = FluidSimulation::new(ctx, config).map_err(js_err)?;
        FLUID.with(|s| *s.borrow_mut() = Some(sim));
        Ok(())
    })
}

/// Advances one step and returns `[height0, height1, ...]`
/// (velocity_y dropped — the site only renders elevation).
#[wasm_bindgen]
pub fn fluid_step() -> Result<Vec<f32>, JsValue> {
    FLUID.with(|s| {
        let mut b = s.borrow_mut();
        let sim = b
            .as_mut()
            .ok_or_else(|| JsValue::from_str("call fluid_start() first"))?;
        let frame = sim.step().map_err(js_err)?;
        Ok(frame.iter().map(|n| n.height).collect())
    })
}

#[wasm_bindgen]
pub fn fluid_set_frequency(hz: f32) -> Result<(), JsValue> {
    FLUID.with(|s| {
        let mut b = s.borrow_mut();
        let sim = b.as_mut().ok_or_else(|| JsValue::from_str("not started"))?;
        sim.set_frequency(hz);
        Ok(())
    })
}

// ---------------------------------------------------------------------
// Acoustic
// ---------------------------------------------------------------------

#[wasm_bindgen]
pub fn acoustic_start(
    width: u32,
    height: u32,
    depth: u32,
    extent: f32,
    frequency_hz: f32,
    seed: u64,
) -> Result<(), JsValue> {
    with_gpu(|ctx| {
        let config = AcousticConfig {
            driving: AcousticDriving {
                frequency_hz,
                amplitude: 1.0,
            },
            medium: MediumSpec {
                density: 1.2,
                sound_speed: 343.0,
            },
            volume: VolumeGrid {
                width,
                height,
                depth,
                extent: [extent, extent, extent],
                readback_stride: 1,
                noise_amplitude: 1e-4,
                seed,
            },
            transducer: TransducerSpec {
                axis: Axis::Z,
                side: Side::Low,
            },
            // 1 mm water droplet in air — the site's documented default.
            particle: ParticleSpec {
                radius: 1.0e-3,
                density: 1000.0,
                sound_speed: 1480.0,
            },
            solver: AcousticSolver {
                dt: 1.0 / (frequency_hz.max(1.0) * 40.0),
                averaging_periods: 8.0,
            },
        };
        let sim = AcousticSimulation::new(ctx, config).map_err(js_err)?;
        ACOUSTIC.with(|s| *s.borrow_mut() = Some(sim));
        Ok(())
    })
}

/// Advances one step and returns `[pressure0, pressure1, ...]` in Pa
/// (force vectors dropped — the site only renders the pressure field).
#[wasm_bindgen]
pub fn acoustic_step() -> Result<Vec<f32>, JsValue> {
    ACOUSTIC.with(|s| {
        let mut b = s.borrow_mut();
        let sim = b
            .as_mut()
            .ok_or_else(|| JsValue::from_str("call acoustic_start() first"))?;
        let frame = sim.step().map_err(js_err)?;
        Ok(frame.iter().map(|n| n.pressure_pa).collect())
    })
}

#[wasm_bindgen]
pub fn acoustic_set_frequency(hz: f32) -> Result<(), JsValue> {
    ACOUSTIC.with(|s| {
        let mut b = s.borrow_mut();
        let sim = b.as_mut().ok_or_else(|| JsValue::from_str("not started"))?;
        sim.set_frequency(hz);
        Ok(())
    })
}
