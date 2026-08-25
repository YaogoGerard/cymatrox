// Granular compute shader — Chladni plate particle dynamics.
//
// Physics per grain (contract § physics model, ADR-0009):
//   w(x,y,t)   = Σ A·sin(mπx/L)·sin(nπy/L)·cos(2π·f·t)
//   F          = −k·∇(|w|²) = −2k·w·∇w     → toward nodal lines
//   semi-implicit Euler: v += F·dt ; v *= exp(−γ·dt) ; p += v·dt
//   walls at [0, L]² with restitution r.
//
// LAYOUT CONTRACT: `Grain`/`ModeEntry`/`Params` mirror the Rust types in
// src/granular/types.rs. Any change here must be applied there in the same
// commit; the assertion tests in that file are the other half of this pact.

struct Grain {
    position: vec2<f32>,
    velocity: vec2<f32>,
}

struct ModeEntry {
    m: u32,
    n: u32,
    omega_hz: f32,
    pad: u32,
}

struct Params {
    plate_size: f32,
    frequency_hz: f32,
    amplitude: f32,
    dt: f32,
    drag: f32,
    restitution: f32,
    coupling_k: f32,
    time: f32,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read_write> grains: array<Grain>;
@group(0) @binding(2) var<storage, read> modes: array<ModeEntry>;

const PI: f32 = 3.14159265358979;
const TAU: f32 = 6.28318530717959;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if (idx >= arrayLength(&grains)) {
        return;
    }

    let L = params.plate_size;
    let g = grains[idx];
    let pi_over_l = PI / L;

    // Accumulate field value and gradient over all resolved modes.
    var w: f32 = 0.0;
    var grad_w: vec2<f32> = vec2<f32>(0.0, 0.0);
    for (var i: u32 = 0u; i < arrayLength(&modes); i = i + 1u) {
        let mo = modes[i];
        let am = f32(mo.m) * pi_over_l;
        let an = f32(mo.n) * pi_over_l;

        let sx = sin(am * g.position.x);
        let cx = cos(am * g.position.x);
        let sy = sin(an * g.position.y);
        let cy = cos(an * g.position.y);
        let phase = cos(TAU * mo.omega_hz * params.time);

        w += params.amplitude * sx * sy * phase;
        grad_w.x += params.amplitude * phase * am * cx * sy;
        grad_w.y += params.amplitude * phase * sx * an * cy;
    }

    // F = −k·∇(|w|²) = −2k·w·∇w
    let force = -params.coupling_k * 2.0 * w * grad_w;

    var v = g.velocity + force * params.dt;
    v = v * exp(-params.drag * params.dt);
    var p = g.position + v * params.dt;

    // Wall rebounds with restitution (O2: positions confined to [0, L]²).
    if (p.x < 0.0) {
        p.x = -p.x;
        v.x = -v.x * params.restitution;
    } else if (p.x > L) {
        p.x = 2.0 * L - p.x;
        v.x = -v.x * params.restitution;
    }
    if (p.y < 0.0) {
        p.y = -p.y;
        v.y = -v.y * params.restitution;
    } else if (p.y > L) {
        p.y = 2.0 * L - p.y;
        v.y = -v.y * params.restitution;
    }
    p = clamp(p, vec2<f32>(0.0), vec2<f32>(L));

    grains[idx].position = p;
    grains[idx].velocity = v;
}
