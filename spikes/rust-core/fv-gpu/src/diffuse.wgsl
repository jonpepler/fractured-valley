fn lds(o: u32) -> I { return I(sides[o], sides[o + 1u]); }
fn ldc(o: u32) -> I { return I(cells[o], cells[o + 1u]); }
// The dense layout (fv-core's cell order): one buffer per kind of data.
// ── layout ────────────────────────────────────────────────────────────────
// A cell (stride 20 words): gas[7], energy, solid_energy, skin.
// A side (stride 25): t, t_cond, t_skin, gas_ml, half, excess, flame_t,
//   flame_heat, flame_fill, fuel_g, reached, water_g, then flags (1 permeable, 2 wet).
// A flux (stride 21): energy, carried, gas[7], flame, then flags (1 present, 2 film).
const CS: u32 = 20u;
const SS: u32 = 29u;
const FS: u32 = 21u;
const ENERGY: u32 = 14u;
const SOLID_E: u32 = 16u;
const SKIN: u32 = 18u;

struct Params {
    nx: u32, ny: u32, nz: u32, bx: u32,
    by: u32, n: u32, lower0: u32, lower1: u32,
    lower2: u32, nlower: u32, ambient_lo: u32, ambient_hi: u32,
    bricks: u32, pad0: u32, pad1: u32, pad2: u32,
}

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var<storage, read> cells: array<u32>;
@group(0) @binding(2) var<storage, read> sides: array<u32>;
// Face velocities, mm/s: 3 per cell (its + faces), then the lower-edge faces.
@group(0) @binding(3) var<storage, read> vel: array<i32>;
@group(0) @binding(4) var<storage, read_write> faces: array<u32>;
@group(0) @binding(5) var<storage, read_write> limits: array<u32>;
@group(0) @binding(6) var<storage, read_write> out_cells: array<u32>;
@group(0) @binding(7) var<storage, read_write> edge: array<u32>;

fn idx(x: u32, y: u32, z: u32) -> u32 {
    let b = ((z / 8u) * P.by + y / 8u) * P.bx + x / 8u;
    return b * 512u + ((z % 8u) * 8u + y % 8u) * 8u + x % 8u;
}
fn xyz(i: u32) -> vec3<u32> {
    let b = i / 512u; let l = i % 512u;
    let o = vec3<u32>(b % P.bx * 8u, b / P.bx % P.by * 8u, b / (P.bx * P.by) * 8u);
    return o + vec3<u32>(l % 8u, l / 8u % 8u, l / 64u);
}

fn side(i: u32) -> Side {
    let o = i * SS;
    var s: Side;
    s.t = lds(o); s.t_cond = lds(o + 2u); s.t_skin = lds(o + 4u); s.gas_ml = lds(o + 6u);
    s.half = lds(o + 8u); s.excess = lds(o + 10u); s.flame_t = lds(o + 12u); s.flame_heat = lds(o + 14u);
    s.flame_fill = lds(o + 16u); s.fuel_g = lds(o + 18u); s.reached = lds(o + 20u); s.water_g = lds(o + 22u);
    s.cap_cond = lds(o + 24u); s.cap_skin = lds(o + 26u);
    let f = sides[o + 28u];
    s.permeable = (f & 1u) != 0u; s.wet = (f & 2u) != 0u;
    return s;
}
fn gas(c: u32, k: u32) -> I { return ldc(c * CS + 2u * k); }
fn ambient() -> I { return I(P.ambient_lo, P.ambient_hi); }
fn store_flux(slot: u32, f: Flux, present: bool) {
    let o = slot * FS;
    faces[o] = f.energy.x; faces[o + 1u] = f.energy.y;
    faces[o + 2u] = f.carried.x; faces[o + 3u] = f.carried.y;
    for (var k = 0u; k < 7u; k++) { faces[o + 4u + 2u * k] = f.gas[k].x; faces[o + 5u + 2u * k] = f.gas[k].y; }
    faces[o + 18u] = f.flame.x; faces[o + 19u] = f.flame.y;
    faces[o + 20u] = select(0u, 1u, present) | select(0u, 2u, f.film);
}
fn fl(o: u32) -> I { return I(faces[o], faces[o + 1u]); }
fn load_flux(slot: u32) -> Flux {
    let o = slot * FS;
    var f: Flux;
    f.energy = fl(o); f.carried = fl(o + 2u);
    for (var k = 0u; k < 7u; k++) { f.gas[k] = fl(o + 4u + 2u * k); }
    f.flame = fl(o + 18u);
    f.film = (faces[o + 20u] & 2u) != 0u;
    return f;
}
fn present(slot: u32) -> bool { return (faces[slot * FS + 20u] & 1u) != 0u; }
fn zero_flux() -> Flux {
    var f: Flux;
    f.energy = I(0u, 0u); f.carried = I(0u, 0u); f.flame = I(0u, 0u); f.film = false;
    for (var k = 0u; k < 7u; k++) { f.gas[k] = I(0u, 0u); }
    return f;
}

fn dims() -> vec3<u32> { return vec3<u32>(P.nx, P.ny, P.nz); }
fn lower_slot(axis: u32, p: vec3<u32>) -> u32 {
    // As Faces::lower: x = 0 by (y, z), y = 0 by (x, z), z = 0 by (x, y).
    if (axis == 0u) { return P.lower0 + p.z * P.ny + p.y; }
    if (axis == 1u) { return P.lower1 + p.z * P.nx + p.x; }
    return P.lower2 + p.y * P.nx + p.x;
}
fn gid(id: vec3<u32>) -> u32 { return id.x + id.y * 65535u * 64u; }

// Faces::new: the + face of each cell along each axis (thread = cell × 3 + axis),
// then the − faces on the box's lower edges.
@compute @workgroup_size(64)
fn faces_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let t = gid(id);
    if (t < P.n * 3u) {
        let i = t / 3u; let axis = t % 3u;
        let p = xyz(i);
        let me = side(i);
        let speed = abs_(i_(vel[t]));
        var q = p; q[axis] += 1u;
        if (q[axis] < dims()[axis]) {
            let n = idx(q.x, q.y, q.z);
            store_flux(t, diffuse_face(i, me, n, side(n), speed, axis), true);
        } else if (axis == 2u || me.permeable) {
            store_flux(t, diffuse_face(i, me, P.n, side(P.n), speed, axis), true);
        } else {
            store_flux(t, zero_flux(), false);
        }
        return;
    }
    let k = t - P.n * 3u;
    if (k >= P.nlower) { return; }
    var axis = 0u; var p = vec3<u32>(0u);
    if (k < P.lower1 - P.n * 3u) { axis = 0u; let j = k; p = vec3<u32>(0u, j % P.ny, j / P.ny); }
    else if (k < P.lower2 - P.n * 3u) { axis = 1u; let j = k - (P.lower1 - P.n * 3u); p = vec3<u32>(j % P.nx, 0u, j / P.nx); }
    else { axis = 2u; let j = k - (P.lower2 - P.n * 3u); p = vec3<u32>(j % P.nx, j / P.nx, 0u); }
    let i = idx(p.x, p.y, p.z);
    let me = side(i);
    let speed = abs_(i_(vel[t]));
    // Below the floor, and beside a solid edge, is a mirror: nothing crosses.
    if (axis != 2u && me.permeable) { store_flux(t, diffuse_face(P.n, side(P.n), i, me, speed, axis), true); }
    else { store_flux(t, zero_flux(), false); }
}

// Faces::get: the face of cell i on side dir of axis: its slot and the cell
// beyond (P.n outside the box); slot 0xffffffff where nothing crosses.
fn face_of(i: u32, p: vec3<u32>, axis: u32, up: bool) -> vec2<u32> {
    if (up) {
        let s = i * 3u + axis;
        if (!present(s)) { return vec2<u32>(0xffffffffu, 0u); }
        var q = p; q[axis] += 1u;
        if (q[axis] < dims()[axis]) { return vec2<u32>(s, idx(q.x, q.y, q.z)); }
        return vec2<u32>(s, P.n);
    }
    if (p[axis] > 0u) {
        var q = p; q[axis] -= 1u;
        let ni = idx(q.x, q.y, q.z);
        let s = ni * 3u + axis;
        if (!present(s)) { return vec2<u32>(0xffffffffu, 0u); }
        return vec2<u32>(s, ni);
    }
    let s = lower_slot(axis, p);
    if (!present(s)) { return vec2<u32>(0xffffffffu, 0u); }
    return vec2<u32>(s, P.n);
}

const FULL: i32 = 1000000;

// transport::gas_limits.
@compute @workgroup_size(64)
fn limits_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = gid(id);
    if (i >= P.n) { return; }
    let p = xyz(i);
    var leaving: array<I, 7>;
    for (var k = 0u; k < 7u; k++) { leaving[k] = I(0u, 0u); }
    for (var axis = 0u; axis < 3u; axis++) {
        for (var d = 0u; d < 2u; d++) {
            let up = d == 1u;
            let fs = face_of(i, p, axis, up);
            if (fs.x == 0xffffffffu) { continue; }
            let o = fs.x * FS;
            for (var k = 0u; k < 7u; k++) {
                let g = fl(o + 4u + 2u * k);
                // change = g × −dir; leaving −= change where negative.
                let change = select(g, neg_(g), up);
                if (isneg(change)) { leaving[k] = sub(leaving[k], change); }
            }
        }
    }
    var ppm = i_(FULL);
    for (var k = 0u; k < 7u; k++) {
        let can = divi(muli(mx(gas(i, k), I(0u, 0u)), 7), 8);
        if (lt(can, leaving[k])) { ppm = mn(ppm, mul_div(can, i_(FULL), leaving[k])); }
    }
    limits[2u * i] = ppm.x; limits[2u * i + 1u] = ppm.y;
}

fn limit(i: u32) -> I { return I(limits[2u * i], limits[2u * i + 1u]); }
fn scale_gas(f: ptr<function, Flux>, ppm: I) {
    if (!lt(ppm, i_(FULL))) { return; }
    for (var k = 0u; k < 7u; k++) { (*f).gas[k] = mul_div_1e6((*f).gas[k], ppm); }
    (*f).carried = mul_div_1e6((*f).carried, ppm);
}

// transport::diffuse_brick, one thread per cell: the cell's new gas and
// energy stores, and what crossed the box's edge through its faces.
@compute @workgroup_size(64)
fn brick_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = gid(id);
    if (i >= P.n) { return; }
    let p = xyz(i);
    let my = side(i);
    var g: array<I, 7>;
    for (var k = 0u; k < 7u; k++) { g[k] = gas(i, k); }
    var energy = ldc(i * CS + ENERGY);
    var solid_e = ldc(i * CS + SOLID_E);
    var skin = ldc(i * CS + SKIN);
    var t_e = I(0u, 0u);
    var t_g: array<I, 7>;
    for (var k = 0u; k < 7u; k++) { t_g[k] = I(0u, 0u); }
    for (var axis = 0u; axis < 3u; axis++) {
        for (var d = 0u; d < 2u; d++) {
            let up = d == 1u;
            let fs = face_of(i, p, axis, up);
            if (fs.x == 0xffffffffu) { continue; }
            var f = load_flux(fs.x);
            let inside = fs.y != P.n;
            if (inside) { scale_gas(&f, mn(limit(i), limit(fs.y))); } else { scale_gas(&f, limit(i)); }
            // apply(next, my, flux, −dir): the lower cell gives (sign −1).
            let e = select(f.energy, neg_(f.energy), up);
            if (my.wet) { solid_e = add(solid_e, e); }
            else if (my.permeable) { energy = add(energy, e); }
            else if (f.film) { skin = add(skin, e); }
            else { solid_e = add(solid_e, e); }
            let flame = select(f.flame, neg_(f.flame), up);
            if (isneg(flame)) { energy = add(energy, flame); } else { skin = add(skin, flame); }
            let c = select(f.carried, neg_(f.carried), up);
            energy = add(energy, c);
            for (var k = 0u; k < 7u; k++) { g[k] = add(g[k], select(f.gas[k], neg_(f.gas[k]), up)); }
            if (!inside) {
                t_e = add(t_e, add(e, c));
                for (var k = 0u; k < 7u; k++) { t_g[k] = add(t_g[k], select(f.gas[k], neg_(f.gas[k]), up)); }
            }
        }
    }
    // A neighbour's flame leaning over this cell's fuel.
    if (gt0(my.fuel_g) && lt(my.t, my.t_skin)) {
        var lean = 0;
        for (var axis = 0u; axis < 2u; axis++) {
            for (var d = 0u; d < 2u; d++) {
                var q = p;
                if (d == 0u) { if (q[axis] == 0u) { continue; } q[axis] -= 1u; }
                else { q[axis] += 1u; if (q[axis] >= dims()[axis]) { continue; } }
                let n = idx(q.x, q.y, q.z);
                if (!iszero(lds(n * SS + 12u))) { lean += C_FLAME_LEAN; }
            }
        }
        lean = min(lean, 1000);
        if (lean > 0) {
            let gg = divi(muli(my.fuel_g, lean), 1000);
            let e0 = ldc(i * CS + ENERGY);
            let latent = muli(gas(i, C_VAPOUR), C_LATENT);
            let q = mn(heat(gg, sub(my.t_skin, my.t)), mx(divi(sub(e0, latent), 4), I(0u, 0u)));
            energy = sub(energy, q);
            skin = add(skin, q);
        }
    }
    let o = i * CS;
    for (var k = 0u; k < 7u; k++) { out_cells[o + 2u * k] = g[k].x; out_cells[o + 2u * k + 1u] = g[k].y; }
    out_cells[o + ENERGY] = energy.x; out_cells[o + ENERGY + 1u] = energy.y;
    out_cells[o + SOLID_E] = solid_e.x; out_cells[o + SOLID_E + 1u] = solid_e.y;
    out_cells[o + SKIN] = skin.x; out_cells[o + SKIN + 1u] = skin.y;
    let eo = i * 16u;
    edge[eo] = t_e.x; edge[eo + 1u] = t_e.y;
    for (var k = 0u; k < 7u; k++) { edge[eo + 2u + 2u * k] = t_g[k].x; edge[eo + 3u + 2u * k] = t_g[k].y; }
}

// Each brick's tally: the sum of its cells' edge crossings (integer sums
// don't depend on their order). Written after the cells' entries in `edge`.
@compute @workgroup_size(64)
fn tally_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let b = gid(id);
    if (b >= P.bricks) { return; }
    var s: array<I, 8>;
    for (var j = 0u; j < 8u; j++) { s[j] = I(0u, 0u); }
    for (var l = 0u; l < 512u; l++) {
        let eo = (b * 512u + l) * 16u;
        for (var j = 0u; j < 8u; j++) { s[j] = add(s[j], I(edge[eo + 2u * j], edge[eo + 2u * j + 1u])); }
    }
    let o = (P.n + b) * 16u;
    for (var j = 0u; j < 8u; j++) { edge[o + 2u * j] = s[j].x; edge[o + 2u * j + 1u] = s[j].y; }
}
