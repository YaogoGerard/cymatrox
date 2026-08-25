// Cymatrox fluid module — parametrically forced damped wave equation
// (ADR-0011). Twin of `src/fluid/types.rs`; update both sides in the same
// commit (ADR-0010 pact).
//
// η_tt = h·g_eff(t)·∇²η − (σh/ρ)∇⁴η − γ·η_t,  g_eff(t) = g + a·cos(ωt)
// Semi-implicit Euler; Dirichlet rim outside the domain mask.

struct Params {
    grid_w: u32,
    grid_h: u32,
    out_w: u32,
    stride: u32,
    dx: f32,
    dy: f32,
    gh_base: f32,        // g·h
    sigma_h_rho: f32,    // σ·h/ρ
    damping_gamma: f32,
    dt: f32,
    drive_omega: f32,    // 2π·f
    drive_accel_h: f32,  // a·h — Mathieu modulation depth
    time: f32,
    radius_sq: f32,      // < 0 disables the circular mask
    centre_x: f32,
    centre_y: f32,
}

struct FluidSurfaceNode {
    height: f32,
    velocity_y: f32,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> src: array<FluidSurfaceNode>;
@group(0) @binding(2) var<storage, read_write> dst: array<FluidSurfaceNode>;
@group(0) @binding(3) var<storage, read_write> out: array<FluidSurfaceNode>;

fn idx(x: u32, y: u32) -> u32 {
    return y * params.grid_w + x;
}

// Dirichlet-aware elevation sample: zero outside the buffer and outside
// the active domain mask.
fn eta_at(x: i32, y: i32) -> f32 {
    if (x < 0 || y < 0 || x >= i32(params.grid_w) || y >= i32(params.grid_h)) {
        return 0.0;
    }
    if (params.radius_sq >= 0.0) {
        let px = f32(x) * params.dx;
        let py = f32(y) * params.dy;
        let ddx = px - params.centre_x;
        let ddy = py - params.centre_y;
        if (ddx * ddx + ddy * ddy > params.radius_sq) {
            return 0.0;
        }
    }
    return src[idx(u32(x), u32(y))].height;
}

// Anisotropic 5-point Laplacian.
fn lap(x: i32, y: i32) -> f32 {
    let c = eta_at(x, y);
    let l = eta_at(x - 1, y);
    let r = eta_at(x + 1, y);
    let d = eta_at(x, y - 1);
    let u = eta_at(x, y + 1);
    return (l + r - 2.0 * c) / (params.dx * params.dx)
         + (u + d - 2.0 * c) / (params.dy * params.dy);
}

fn inside_mask(x: u32, y: u32) -> bool {
    if (params.radius_sq < 0.0) {
        return true;
    }
    let px = f32(x) * params.dx;
    let py = f32(y) * params.dy;
    let ddx = px - params.centre_x;
    let ddy = py - params.centre_y;
    return ddx * ddx + ddy * ddy <= params.radius_sq;
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= params.grid_w || gid.y >= params.grid_h) {
        return;
    }
    let xi = i32(gid.x);
    let yi = i32(gid.y);

    // Second application of the Laplacian (the ∇⁴ capillary term).
    let b = (lap(xi - 1, yi) + lap(xi + 1, yi) - 2.0 * lap(xi, yi)) / (params.dx * params.dx)
          + (lap(xi, yi + 1) + lap(xi, yi - 1) - 2.0 * lap(xi, yi)) / (params.dy * params.dy);

    let cur = src[idx(gid.x, gid.y)];

    // Mathieu modulation of the gravity coefficient.
    let gh_eff = params.gh_base + params.drive_accel_h * cos(params.drive_omega * params.time);
    let accel = gh_eff * lap(xi, yi) - params.sigma_h_rho * b - params.damping_gamma * cur.velocity_y;

    var v_new = cur.velocity_y + params.dt * accel;
    var e_new = cur.height + params.dt * v_new;

    if (!inside_mask(gid.x, gid.y)) {
        v_new = 0.0;
        e_new = 0.0;
    }

    dst[idx(gid.x, gid.y)] = FluidSurfaceNode(e_new, v_new);

    // Compacted strided output (contract O1).
    if (gid.x % params.stride == 0u && gid.y % params.stride == 0u) {
        let oidx = (gid.y / params.stride) * params.out_w + gid.x / params.stride;
        out[oidx] = FluidSurfaceNode(e_new, v_new);
    }
}
