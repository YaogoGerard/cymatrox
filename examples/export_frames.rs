//! Exports real cymatrox simulation frames to JSON for the website's
//! data-driven viewer (website/index.html).
//!
//! Run from the repo root:
//!
//! ```sh
//! cargo run --release --example export_frames
//! gzip -kf website/data/*.json   # site prefers .json.gz (DecompressionStream)
//! ```
//!
//! Uses ONLY the published API of cymatrox v0.1.0 — this example adds no
//! physics and no new dependencies: the JSON is written by hand via
//! `std::fmt::Write` so the crate's dependency graph stays untouched.
//!
//! Requires a GPU-capable host (ADR-0008).

use std::fmt::Write as _;

use cymatrox::acoustic::{
    AcousticConfig, AcousticSimulation, Axis, Driving as AcousticDriving, MediumSpec, ParticleSpec,
    Side, SolverParams as AcousticSolver, TransducerSpec, VolumeGrid,
};
use cymatrox::fluid::{
    DomainMask, DomainShape, Driving as FluidDriving, FluidConfig, FluidSimulation, LiquidSpec,
    SolverParams as FluidSolver, SurfaceGrid,
};
use cymatrox::granular::{
    Driving as GranularDriving, GrainBed, GranularConfig, GranularSimulation, InitialDistribution,
    ModeSelection, PlateSpec, SolverParams as GranularSolver,
};
use cymatrox::{GpuContext, Result};

fn main() -> Result<()> {
    std::fs::create_dir_all("website/data").expect("create website/data");
    let ctx = poll_gpu();

    export_granular(&ctx)?;
    export_fluid(&ctx)?;
    export_acoustic(&ctx)?;

    for f in ["granular", "fluid", "acoustic"] {
        let path = format!("website/data/{f}.json");
        let bytes = std::fs::metadata(&path).expect(&path).len();
        println!("{path}: {} KiB", bytes / 1024);
    }
    Ok(())
}

/// `GpuContext::new()` is the only async call in the whole API; a tiny
/// hand-rolled reactor is enough here — no runtime dependency wanted.
fn poll_gpu() -> GpuContext {
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    fn noop(_: *const ()) {}
    fn clone(p: *const ()) -> RawWaker {
        RawWaker::new(p, &VTABLE)
    }
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);

    let fut = GpuContext::new();
    let mut fut = Box::pin(fut);
    let raw = RawWaker::new(std::ptr::null(), &VTABLE);
    let waker = unsafe { Waker::from_raw(raw) };
    let mut cx = Context::from_waker(&waker);
    loop {
        // The adapter request completes after one or few executor ticks.
        if let Poll::Ready(res) = fut.as_mut().poll(&mut cx) {
            return res.expect("GPU context");
        }
        std::hint::spin_loop();
    }
}

// ---------------------------------------------------------------------
// Granular — Chladni pattern formation, 48 sampled frames over ~1 s.
// ---------------------------------------------------------------------

fn export_granular(ctx: &GpuContext) -> Result<()> {
    const COUNT: u32 = 5_000;
    const STEPS: u32 = 320;
    const EVERY: u32 = 10;

    let config = GranularConfig {
        experiment: GranularDriving {
            frequency_hz: 440.0,
            amplitude: 1e-4,
            modes: ModeSelection::Auto,
        },
        medium: PlateSpec::Idealized { side: 0.5 },
        grains: GrainBed {
            count: COUNT,
            distribution: InitialDistribution::Uniform,
            seed: 42,
        },
        solver: GranularSolver {
            dt: 1.0 / 480.0,
            drag: 4.0,
            restitution: 0.6,
            coupling_k: 5.0e5,
            base_frequency_hz: 120.0,
        },
    };

    let mut sim = GranularSimulation::new(ctx, config)?;
    let mut frames_json: Vec<String> = Vec::new();
    for step in 1..=STEPS {
        let frame = sim.step()?;
        if step % EVERY == 0 {
            let mut s = String::with_capacity(COUNT as usize * 14);
            s.push('[');
            for g in frame.iter() {
                write!(s, "{:.3},{:.3},", g.position[0], g.position[1]).ok();
            }
            s.pop(); // trailing comma
            s.push(']');
            frames_json.push(s);
        }
    }

    let mut json = String::with_capacity(1 << 23);
    write!(
        json,
        r#"{{"module":"granular","meta":{{"count":{COUNT},"side":0.5,"frequency_hz":440,"dt":{:.10},"steps_between_frames":{EVERY},"frame_count":{}}},"frames":["#,
        1.0 / 480.0,
        frames_json.len()
    )
    .ok();
    json.push_str(&frames_json.join(","));
    json.push_str("]}");
    std::fs::write("website/data/granular.json", json).expect("write granular.json");
    println!("granular: {} frames x {COUNT} grains", frames_json.len());
    Ok(())
}

// ---------------------------------------------------------------------
// Fluid — Faraday ripples in a circular dish, 90 frames over 2 periods.
// ---------------------------------------------------------------------

fn export_fluid(ctx: &GpuContext) -> Result<()> {
    const W: u32 = 96;
    const H: u32 = 96;
    const STRIDE: u32 = 2;
    const STEPS: u32 = 900;
    const EVERY: u32 = 10;

    let config = FluidConfig {
        driving: FluidDriving {
            frequency_hz: 60.0,
            amplitude: 90.0,
        },
        liquid: LiquidSpec {
            density: 1000.0,
            surface_tension: 0.072,
            depth: 0.004,
            damping: 0.8,
            gravity: 9.81,
        },
        surface: SurfaceGrid {
            width: W,
            height: H,
            extent: [0.06, 0.06],
            readback_stride: STRIDE,
            noise_amplitude: 1e-7,
            seed: 7,
        },
        domain: DomainMask {
            shape: DomainShape::Circular { radius: 0.025 },
        },
        solver: FluidSolver { dt: 4e-5 },
    };

    let mut sim = FluidSimulation::new(ctx, config)?;
    let ox = W.div_ceil(STRIDE);
    let oy = H.div_ceil(STRIDE);

    let mut frames_json: Vec<String> = Vec::new();
    for step in 1..=STEPS {
        let frame = sim.step()?;
        if step % EVERY == 0 {
            let mut s = String::with_capacity(frame.len() * 8);
            s.push('[');
            for n in frame.iter() {
                write!(s, "{:.4},", n.height).ok();
            }
            s.pop();
            s.push(']');
            frames_json.push(s);
        }
    }

    let mut json = String::with_capacity(1 << 21);
    write!(
        json,
        r#"{{"module":"fluid","meta":{{"out_x":{ox},"out_y":{oy},"stride":{STRIDE},"extent":[0.06,0.06],"radius":0.025,"frequency_hz":60,"dt":4e-05,"steps_between_frames":{EVERY},"frame_count":{}}},"frames":["#,
        frames_json.len()
    )
    .ok();
    json.push_str(&frames_json.join(","));
    json.push_str("]}");
    std::fs::write("website/data/fluid.json", json).expect("write fluid.json");
    println!("fluid: {} frames x {ox}x{oy} nodes", frames_json.len());
    Ok(())
}

// ---------------------------------------------------------------------
// Acoustic — standing-wave pressure in a 32³ cavity, mid-plane slices.
// ---------------------------------------------------------------------

fn export_acoustic(ctx: &GpuContext) -> Result<()> {
    const N: u32 = 32;
    const STRIDE: u32 = 2;
    const STEPS: u32 = 360;
    const EVERY: u32 = 5;

    let config = AcousticConfig {
        driving: AcousticDriving {
            frequency_hz: 24_000.0,
            amplitude: 5.0,
        },
        medium: MediumSpec {
            density: 1.2041,
            sound_speed: 343.0,
        },
        volume: VolumeGrid {
            width: N,
            height: N,
            depth: N,
            extent: [0.04; 3],
            readback_stride: STRIDE,
            noise_amplitude: 1e-9,
            seed: 123,
        },
        transducer: TransducerSpec {
            axis: Axis::X,
            side: Side::Low,
        },
        particle: ParticleSpec {
            radius: 1e-3,
            density: 1000.0,
            sound_speed: 1480.0,
        },
        solver: AcousticSolver {
            dt: 4e-7,
            averaging_periods: 8.0,
        },
    };

    let mut sim = AcousticSimulation::new(ctx, config)?;
    let on = N.div_ceil(STRIDE); // output dims per axis

    let mut frames_json: Vec<String> = Vec::new();
    for step in 1..=STEPS {
        let frame = sim.step()?;
        if step % EVERY == 0 {
            // Full strided volume per frame; the site slices it freely.
            let mut s = String::with_capacity(frame.len() * 8);
            s.push('[');
            for nd in frame.iter() {
                write!(s, "{:.2},", nd.pressure_pa).ok();
            }
            s.pop();
            s.push(']');
            frames_json.push(s);
        }
    }

    let mut json = String::with_capacity(1 << 21);
    write!(
        json,
        r#"{{"module":"acoustic","meta":{{"out_x":{on},"out_y":{on},"out_z":{on},"stride":{STRIDE},"extent":0.04,"frequency_hz":24000,"dt":4e-07,"steps_between_frames":{EVERY},"frame_count":{}}},"frames":["#,
        frames_json.len()
    )
    .ok();
    json.push_str(&frames_json.join(","));
    json.push_str("]}");
    std::fs::write("website/data/acoustic.json", json).expect("write acoustic.json");
    println!("acoustic: {} frames x {on}³ nodes", frames_json.len());
    Ok(())
}
