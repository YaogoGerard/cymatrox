// Cymatrox acoustic module — time-domain wave propagation with Gor'kov
// forces (ADR-0012). Twin of `src/acoustic/types.rs`; update both sides
// in the same commit (ADR-0010 pact).
//
//   p_t = q ;  q_t = c²∇²p      (semi-implicit Euler)
//
// Rigid walls via mirrored ghost cells; the transducer face imposes a
// Neumann velocity condition. Per-node EMA accumulators of p² and |∇p|²
// feed the Gor'kov force F = −∇U in the second dispatch.
//
// NOTE: the force member is `array<f32,3>`, NOT `vec3<f32>` — vec3 would
// force 16-byte alignment and grow the node struct to 32 bytes.

struct Params {
    grid_x: u32,
    grid_y: u32,
    grid_z: u32,
    out_x: u32,
    out_y: u32,
    stride: u32,
    axis: i32,
    side: i32,
    dx: f32,
    dy: f32,
    dz: f32,
    c2: f32,
    rho0: f32,
    dt: f32,
    omega: f32,
    drive_u: f32,
    time: f32,
    ema_alpha: f32,
    neumann_amp: f32,
    _pad0: f32,
    gk_p_coeff: f32,
    gk_g_coeff: f32,
    _pad1: f32,
    _pad2: f32,
}

struct StateNode {
    p: f32,
    q: f32,
}

struct AvgAccum {
    p2: f32,
    g2: f32,
}

struct AcousticPressureNode {
    pressure_pa: f32,
    force: array<f32, 3>,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> src: array<StateNode>;
@group(0) @binding(2) var<storage, read_write> dst: array<StateNode>;
@group(0) @binding(3) var<storage, read_write> avg: array<AvgAccum>;
@group(0) @binding(4) var<storage, read_write> out: array<AcousticPressureNode>;

fn idx(x: u32, y: u32, z: u32) -> u32 {
    return (z * params.grid_y + y) * params.grid_x + x;
}

// Neumann forcing on the driven face: ∂p/∂n = ρ₀·ω·u₀·sin(ωt).
fn forcing() -> f32 {
    return params.neumann_amp * sin(params.omega * params.time);
}

// `refl_axis` tells which stencil axis produced the ghost call (0=x,
// 1=y, 2=z) — a coordinate being out of range alone does NOT mean the
// transducer face was hit (twin of oracle `driven_face(axis_is_x,…)`).
fn is_driven_face(refl_axis: i32, coord: i32, limit: i32) -> bool {
    if (refl_axis != params.axis) {
        return false;
    }
    if (coord < 0) {
        return params.side == 0i;
    }
    if (coord >= limit) {
        return params.side == 1i;
    }
    return false;
}

// Dirichlet-free Neumann-aware pressure sample. Ghost cells mirror the
// inner neighbour; the driven face adds the forcing correction.
fn p_at(x: i32, y: i32, z: i32) -> f32 {
    let nx = i32(params.grid_x);
    let ny = i32(params.grid_y);
    let nz = i32(params.grid_z);

    if (x < 0 || x >= nx) {
        let sgn: f32 = select(-1.0, 1.0, x >= 0);
        if (is_driven_face(0i, x, nx)) {
            let ix: i32 = select(nx - 2, 1, x < 0);
            return src[idx(u32(ix), u32(y), u32(z))].p - sgn * 2.0 * params.dx * forcing();
        }
        let ix: i32 = select(nx - 2, 1, x < 0);
        return src[idx(u32(ix), u32(y), u32(z))].p;
    }
    if (y < 0 || y >= ny) {
        let sgn: f32 = select(-1.0, 1.0, y >= 0);
        if (is_driven_face(1i, y, ny)) {
            let iy: i32 = select(ny - 2, 1, y < 0);
            return src[idx(u32(x), u32(iy), u32(z))].p - sgn * 2.0 * params.dy * forcing();
        }
        let iy: i32 = select(ny - 2, 1, y < 0);
        return src[idx(u32(x), u32(iy), u32(z))].p;
    }
    if (z < 0 || z >= nz) {
        let sgn: f32 = select(-1.0, 1.0, z >= 0);
        if (is_driven_face(2i, z, nz)) {
            let iz: i32 = select(nz - 2, 1, z < 0);
            return src[idx(u32(x), u32(y), u32(iz))].p - sgn * 2.0 * params.dz * forcing();
        }
        let iz: i32 = select(nz - 2, 1, z < 0);
        return src[idx(u32(x), u32(y), u32(iz))].p;
    }
    return src[idx(u32(x), u32(y), u32(z))].p;
}

/// Dispatch 1 — wave update + EMA accumulators.
@compute @workgroup_size(4, 4, 4)
fn wave_step(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= params.grid_x || gid.y >= params.grid_y || gid.z >= params.grid_z) {
        return;
    }
    let xi = i32(gid.x);
    let yi = i32(gid.y);
    let zi = i32(gid.z);

    // 7-point Laplacian through the Neumann-aware sampler.
    let lap =
        (p_at(xi - 1, yi, zi) + p_at(xi + 1, yi, zi) - 2.0 * p_at(xi, yi, zi)) / (params.dx * params.dx)
        + (p_at(xi, yi - 1, zi) + p_at(xi, yi + 1, zi) - 2.0 * p_at(xi, yi, zi)) / (params.dy * params.dy)
        + (p_at(xi, yi, zi - 1) + p_at(xi, yi, zi + 1) - 2.0 * p_at(xi, yi, zi)) / (params.dz * params.dz);

    let cur = src[idx(gid.x, gid.y, gid.z)];
    let q_new = cur.q + params.dt * params.c2 * lap;
    let p_new = cur.p + params.dt * q_new;

    dst[idx(gid.x, gid.y, gid.z)] = StateNode(p_new, q_new);

    // EMA of p² and |∇p|² — gradient from the pre-update field (contract I2).
    let gx = (p_at(xi + 1, yi, zi) - p_at(xi - 1, yi, zi)) / (2.0 * params.dx);
    let gy = (p_at(xi, yi + 1, zi) - p_at(xi, yi - 1, zi)) / (2.0 * params.dy);
    let gz = (p_at(xi, yi, zi + 1) - p_at(xi, yi, zi - 1)) / (2.0 * params.dz);
    let a = params.ema_alpha;
    let keep = 1.0 - a;
    let old = avg[idx(gid.x, gid.y, gid.z)];
    avg[idx(gid.x, gid.y, gid.z)] = AvgAccum(
        keep * old.p2 + a * p_new * p_new,
        keep * old.g2 + a * (gx * gx + gy * gy + gz * gz),
    );
}

// Gor'kov potential from accumulated cycle averages at one node.
fn u_at(x: u32, y: u32, z: u32) -> f32 {
    let acc = avg[idx(x, y, z)];
    return params.gk_p_coeff * acc.p2 + params.gk_g_coeff * acc.g2;
}

/// Dispatch 2 — Gor'kov forces + compacted strided output.
@compute @workgroup_size(4, 4, 4)
fn gorkov(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= params.grid_x || gid.y >= params.grid_y || gid.z >= params.grid_z) {
        return;
    }

    // Clamped neighbour coordinates for the U gradient.
    let mx = params.grid_x - 1u;
    let my = params.grid_y - 1u;
    let mz = params.grid_z - 1u;
    let xp = u32(min(i32(gid.x) + 1, i32(mx)));
    let xm = u32(max(i32(gid.x) - 1, 0));
    let yp = u32(min(i32(gid.y) + 1, i32(my)));
    let ym = u32(max(i32(gid.y) - 1, 0));
    let zp = u32(min(i32(gid.z) + 1, i32(mz)));
    let zm = u32(max(i32(gid.z) - 1, 0));

    let fx = -(u_at(xp, gid.y, gid.z) - u_at(xm, gid.y, gid.z)) / (f32(xp - xm) * params.dx);
    let fy = -(u_at(gid.x, yp, gid.z) - u_at(gid.x, ym, gid.z)) / (f32(yp - ym) * params.dy);
    let fz = -(u_at(gid.x, gid.y, zp) - u_at(gid.x, gid.y, zm)) / (f32(zp - zm) * params.dz);

    // Compacted strided output (contract O1): x fastest, then y, then z.
    if (gid.x % params.stride == 0u && gid.y % params.stride == 0u && gid.z % params.stride == 0u) {
        let oidx = (gid.z / params.stride) * params.out_x * params.out_y
                 + (gid.y / params.stride) * params.out_x
                 + gid.x / params.stride;
        out[oidx] = AcousticPressureNode(dst[idx(gid.x, gid.y, gid.z)].p, array<f32, 3>(fx, fy, fz));
    }
}
