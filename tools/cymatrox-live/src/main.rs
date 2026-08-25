//! Local live server for the cymatrox website.
//!
//! Serves `website/` as static files and exposes a tiny HTTP API that runs
//! REAL simulations through the published `cymatrox` crate on the local GPU,
//! returning frames as JSON (same schema as `examples/export_frames.rs`).
//!
//! ```sh
//! cargo run --release          # inside tools/cymatrox-live
//! # open http://127.0.0.1:8030
//! ```
//!
//! This is a standalone consumer of crates.io `cymatrox = "0.1"` — the
//! published crate itself is never modified.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;

use std::sync::{Arc, Mutex};

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

const SITE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../website");
const DEFAULT_PORT: u16 = 8030;
const CYMATROX_CRATE_VERSION: &str = "0.1";

fn main() {
    let port: u16 = std::env::var("CYMATROX_LIVE_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT);
    let addr = format!("127.0.0.1:{port}");

    println!("cymatrox-live — initialisation GPU…");
    let ctx = poll_gpu();
    println!("GPU prêt.");

    let ctx = Arc::new(Mutex::new(ctx));
    let server = Arc::new(tiny_http::Server::http(&addr).expect("bind server"));
    println!("API + site sur http://{addr}  —  Ctrl+C pour arrêter");

    let mut workers = Vec::new();
    for _ in 0..4 {
        let (srv, cx) = (server.clone(), ctx.clone());
        workers.push(std::thread::spawn(move || loop {
            match srv.recv() {
                Ok(req) => handle(req, &cx),
                Err(e) => {
                    eprintln!("recv error: {e}");
                    break;
                }
            }
        }));
    }
    for w in workers {
        let _ = w.join();
    }
}

/// `GpuContext::new()` is the only async call in the whole API; a tiny
/// hand-rolled reactor is enough — no runtime dependency wanted.
fn poll_gpu() -> GpuContext {
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    fn noop(_: *const ()) {}
    fn clone(p: *const ()) -> RawWaker {
        RawWaker::new(p, &VTABLE)
    }
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);

    use std::future::Future;
    let fut = GpuContext::new();
    let mut fut = Box::pin(fut);
    let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
    let mut cx = Context::from_waker(&waker);
    loop {
        if let Poll::Ready(res) = fut.as_mut().poll(&mut cx) {
            return res.expect("GPU context");
        }
        std::hint::spin_loop();
    }
}

// ---------------------------------------------------------------------
// HTTP plumbing
// ---------------------------------------------------------------------

enum Body {
    Text(String),
    Bytes(Vec<u8>),
}

type Reply = (Body, &'static str, u16);

fn handle(req: tiny_http::Request, ctx: &Mutex<GpuContext>) {
    let url = req.url().to_string();
    let (path, query) = match url.split_once('?') {
        Some((p, q)) => (p.to_string(), parse_query(q)),
        None => (url.clone(), HashMap::new()),
    };

    let t0 = std::time::Instant::now();
    let reply = route(&path, &query, ctx);
    let ms = t0.elapsed().as_millis();
    println!("{} -> {} ({} ms)", path, reply.2, ms);

    let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], reply.1.as_bytes()).unwrap();
    let response = match reply.0 {
        Body::Text(s) => tiny_http::Response::from_string(s),
        Body::Bytes(b) => tiny_http::Response::from_data(b),
    };
    req.respond(response.with_header(header).with_status_code(reply.2))
        .ok();
}

fn route(path: &str, q: &HashMap<String, String>, ctx: &Mutex<GpuContext>) -> Reply {
    match path {
        "/api/ping" => ok_text(format!(
            r#"{{"ok":true,"crate":"cymatrox","version":"{CYMATROX_CRATE_VERSION}"}}"#
        )),
        "/api/granular" => sim_reply("granular", run_granular(q, ctx)),
        "/api/fluid" => sim_reply("fluid", run_fluid(q, ctx)),
        "/api/acoustic" => sim_reply("acoustic", run_acoustic(q, ctx)),
        _ => static_file(path),
    }
}

fn ok_text(body: String) -> Reply {
    (Body::Text(body), "application/json", 200)
}

fn sim_reply(module: &str, inner: Result<String>) -> Reply {
    match inner {
        Ok(body) => (Body::Text(body), "application/json", 200),
        Err(e) => (
            Body::Text(format!(
                r#"{{"error":"{module} failed","detail":"{}"}}"#,
                escape_js(&e.to_string())
            )),
            "application/json",
            500,
        ),
    }
}

fn escape_js(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', " ")
}

/// Minimal query-string parser (`a=1&b=2`, no percent-decoding needed:
/// every parameter we accept is numeric).
fn parse_query(q: &str) -> HashMap<String, String> {
    q.split('&')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn num(q: &HashMap<String, String>, key: &str, default: f64) -> f64 {
    q.get(key).and_then(|v| v.parse().ok()).unwrap_or(default)
}
fn uint(q: &HashMap<String, String>, key: &str, default: u32, min: u32, max: u32) -> u32 {
    num(q, key, default as f64).clamp(min as f64, max as f64) as u32
}

/// Keep the frame count bounded so responses stay fast even if the caller
/// asks for many steps with a small cadence.
fn effective_every(steps: u32, every: u32, max_frames: u32) -> u32 {
    let mut e = every.max(1);
    while steps / e > max_frames {
        e += 1;
    }
    e
}

// ---------------------------------------------------------------------
// Static files
// ---------------------------------------------------------------------

fn static_file(path: &str) -> Reply {
    let rel = if path == "/" { "/index.html" } else { path };
    let root = Path::new(SITE_ROOT);
    let full = root.join(rel.trim_start_matches('/'));
    if !safe_under(root, &full) {
        return (Body::Text("forbidden".into()), "text/plain", 403);
    }
    match std::fs::read(&full) {
        Ok(bytes) => (Body::Bytes(bytes), mime_of(rel), 200),
        Err(_) => (Body::Text("not found".into()), "text/plain", 404),
    }
}

fn safe_under(root: &Path, candidate: &Path) -> bool {
    let Ok(c) = candidate.canonicalize() else {
        // Missing files fall through to the read-failure path; only reject
        // existing-but-outside paths here.
        return true;
    };
    let Ok(r) = root.canonicalize() else {
        return false;
    };
    c.starts_with(r)
}

fn mime_of(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html",
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "json" => "application/json",
        "gz" => "application/gzip",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    }
}

// ---------------------------------------------------------------------
// Simulations — real physics through the published cymatrox API
// ---------------------------------------------------------------------

fn run_granular(q: &HashMap<String, String>, ctx: &Mutex<GpuContext>) -> Result<String> {
    const MAX_FRAMES: u32 = 240;

    let freq = num(q, "f", 440.0).clamp(1.0, 1.0e6);
    let n = uint(q, "n", 5, 1, 20);
    let m = uint(q, "m", 3, 0, 20);
    let count = uint(q, "count", 5_000, 100, 20_000);
    let side = num(q, "side", 0.5);
    let steps = uint(q, "steps", 320, 10, 3_000);
    let every = effective_every(steps, uint(q, "every", 10, 1, 500), MAX_FRAMES);
    let auto_modes = q.get("modes").map(|v| v == "auto").unwrap_or(false);

    let config = GranularConfig {
        experiment: GranularDriving {
            frequency_hz: freq as f32,
            amplitude: 1e-4,
            modes: if auto_modes {
                ModeSelection::Auto
            } else {
                ModeSelection::Explicit(vec![(m, n)])
            },
        },
        medium: PlateSpec::Idealized { side: side as f32 },
        grains: GrainBed {
            count,
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

    let ctx = ctx.lock().expect("gpu mutex");
    let mut sim = GranularSimulation::new(&ctx, config)?;

    let mut frames: Vec<String> = Vec::new();
    for step in 1..=steps {
        let frame = sim.step()?;
        if step % every == 0 {
            let mut s = String::with_capacity(count as usize * 14);
            s.push('[');
            for g in frame.iter() {
                write!(s, "{:.3},{:.3},", g.position[0], g.position[1]).ok();
            }
            s.pop();
            s.push(']');
            frames.push(s);
        }
    }

    let mut out = String::with_capacity(1 << 22);
    write!(
        out,
        r#"{{"module":"granular","meta":{{"count":{count},"side":{side},"frequency_hz":{freq},"dt":{:.10},"steps_between_frames":{every},"frame_count":{}}},"frames":["#,
        1.0 / 480.0,
        frames.len()
    )
    .ok();
    out.push_str(&frames.join(","));
    out.push_str("]}");
    Ok(out)
}

fn run_fluid(q: &HashMap<String, String>, ctx: &Mutex<GpuContext>) -> Result<String> {
    const MAX_FRAMES: u32 = 240;

    let freq = num(q, "f", 60.0).clamp(0.1, 1.0e6);
    let amp = num(q, "amp", 90.0).clamp(0.0, 1.0e4);
    let grid = uint(q, "grid", 96, 16, 160);
    let extent = num(q, "extent", 0.06);
    let radius = num(q, "radius", 0.025);
    let steps = uint(q, "steps", 900, 10, 4_000);
    let stride = uint(q, "stride", 2, 1, 8);
    let every = effective_every(steps, uint(q, "every", 10, 1, 500), MAX_FRAMES);

    let config = FluidConfig {
        driving: FluidDriving {
            frequency_hz: freq as f32,
            amplitude: amp as f32,
        },
        liquid: LiquidSpec {
            density: 1000.0,
            surface_tension: 0.072,
            depth: 0.004,
            damping: 0.8,
            gravity: 9.81,
        },
        surface: SurfaceGrid {
            width: grid,
            height: grid,
            extent: [extent as f32, extent as f32],
            readback_stride: stride,
            noise_amplitude: 1e-7,
            seed: 7,
        },
        domain: DomainMask {
            shape: DomainShape::Circular {
                radius: radius as f32,
            },
        },
        solver: FluidSolver { dt: 4e-5 },
    };

    let ctx = ctx.lock().expect("gpu mutex");
    let mut sim = FluidSimulation::new(&ctx, config)?;
    let ox = grid.div_ceil(stride);
    let oy = grid.div_ceil(stride);

    let mut frames: Vec<String> = Vec::new();
    for step in 1..=steps {
        let frame = sim.step()?;
        if step % every == 0 {
            let mut s = String::with_capacity(frame.len() * 8);
            s.push('[');
            for nd in frame.iter() {
                write!(s, "{:.4},", nd.height).ok();
            }
            s.pop();
            s.push(']');
            frames.push(s);
        }
    }

    let mut out = String::with_capacity(1 << 21);
    write!(
        out,
        r#"{{"module":"fluid","meta":{{"out_x":{ox},"out_y":{oy},"stride":{stride},"extent":[{extent},{extent}],"radius":{radius},"frequency_hz":{freq},"dt":4e-05,"steps_between_frames":{every},"frame_count":{}}},"frames":["#,
        frames.len()
    )
    .ok();
    out.push_str(&frames.join(","));
    out.push_str("]}");
    Ok(out)
}

fn run_acoustic(q: &HashMap<String, String>, ctx: &Mutex<GpuContext>) -> Result<String> {
    const MAX_FRAMES: u32 = 240;

    let freq = num(q, "f", 24_000.0).clamp(100.0, 2.0e6);
    let amp = num(q, "amp", 5.0).clamp(0.0, 1.0e3);
    let n = uint(q, "grid", 32, 16, 48);
    let extent = num(q, "extent", 0.04);
    let steps = uint(q, "steps", 360, 10, 2_000);
    let stride = uint(q, "stride", 2, 1, 4);
    let every = effective_every(steps, uint(q, "every", 5, 1, 200), MAX_FRAMES);

    let config = AcousticConfig {
        driving: AcousticDriving {
            frequency_hz: freq as f32,
            amplitude: amp as f32,
        },
        medium: MediumSpec {
            density: 1.2041,
            sound_speed: 343.0,
        },
        volume: VolumeGrid {
            width: n,
            height: n,
            depth: n,
            extent: [extent as f32; 3],
            readback_stride: stride,
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

    let ctx = ctx.lock().expect("gpu mutex");
    let mut sim = AcousticSimulation::new(&ctx, config)?;
    let on = n.div_ceil(stride);

    let mut frames: Vec<String> = Vec::new();
    for step in 1..=steps {
        let frame = sim.step()?;
        if step % every == 0 {
            let mut s = String::with_capacity(frame.len() * 8);
            s.push('[');
            for nd in frame.iter() {
                write!(s, "{:.2},", nd.pressure_pa).ok();
            }
            s.pop();
            s.push(']');
            frames.push(s);
        }
    }

    let mut out = String::with_capacity(1 << 21);
    write!(
        out,
        r#"{{"module":"acoustic","meta":{{"out_x":{on},"out_y":{on},"out_z":{on},"stride":{stride},"extent":{extent},"frequency_hz":{freq},"dt":4e-07,"steps_between_frames":{every},"frame_count":{}}},"frames":["#,
        frames.len()
    )
    .ok();
    out.push_str(&frames.join(","));
    out.push_str("]}");
    Ok(out)
}
