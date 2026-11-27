// The diffusion phase on a brick pool (brief 10, D3): the world is a pool of
// 8³ bricks in any order, each with a 26-neighbour slot table, its stores in
// field groups (gas, heat), struct-of-arrays inside each brick, and the
// state double-buffered. No face table in global memory: each workgroup
// takes a 4³ tile, works out its faces one axis at a time in workgroup
// memory, and uses them at once. Two passes, both from the previous state:
//   1. limits_pass: each cell's gas limiter (transport::gas_limits);
//   2. apply_pass: every face again, scaled by the limiters, applied to each
//      cell, plus the lean of neighbouring flames and the tile's edge tally.
// A missing neighbour brick is the box's edge, under fv-core's rules (the
// floor is a mirror; the top, and a side beside open air, is outside air).

struct Params {
    slots: u32, in_copy: u32, out_copy: u32, copy_words: u32,
    heat_base: u32, vlow: u32, outside: u32, ambient_lo: u32,
    ambient_hi: u32, tiles_x: u32, compact: u32, flag: u32,
    // Carrying: sub-steps per tick, the outside air's gas temperature, and
    // whether this sub-step adds to the tallies (else it starts them).
    steps: u32, outside_t_lo: u32, outside_t_hi: u32, accumulate: u32,
}
@group(0) @binding(0) var<uniform> P: Params;
// Two copies of the state; in each, the gas group (7 fields × 512 cells per
// slot, i64 as two words) and then the heat group (energy, solid_energy, skin).
@group(0) @binding(1) var<storage, read_write> state: array<u32>;
// Each cell's side (25 words, worked out on the CPU for now).
@group(0) @binding(2) var<storage, read_write> sides_rw: array<u32>;
// Face velocities: each cell's three + faces (per slot, per axis), then each
// slot's lower boundary faces (3 × 64).
@group(0) @binding(3) var<storage, read> vel: array<i32>;
// Per slot, 26 neighbour slots (0xffffffff: none), then the outside cell
// (20 words: gas, energy, solid_energy, skin) and its side (25).
@group(0) @binding(4) var<storage, read> table: array<u32>;
@group(0) @binding(5) var<storage, read_write> limits: array<u32>;
// Per tile: what crossed the box's edge (energy, then 7 gases).
@group(0) @binding(6) var<storage, read_write> tallies: array<u32>;

const OUT: u32 = 0xffffffffu;
const NONE: u32 = 0xfffffffeu;
const SS: u32 = 25u;

fn ambient() -> I { return I(P.ambient_lo, P.ambient_hi); }
fn st(o: u32) -> I { return I(state[o], state[o + 1u]); }
// The gas group: i64 (two words) per mass, or in the compact cell u32 (one
// word; a mass is never negative, and the host and shader check < 2³²).
fn gas_word(copy: u32, c: u32, k: u32) -> u32 {
    let slot = c / 512u; let l = c % 512u;
    return copy * P.copy_words + select(2u, 1u, P.compact != 0u) * ((slot * 7u + k) * 512u + l);
}
fn gas_at(copy: u32, c: u32, k: u32) -> I {
    let o = gas_word(copy, c, k);
    if (P.compact != 0u) { return I(state[o], 0u); }
    return st(o);
}
fn put_gas(copy: u32, c: u32, k: u32, v: I) {
    let o = gas_word(copy, c, k);
    if (P.compact != 0u) {
        // Out of the compact range: flag it (the host refuses the result).
        if (v.y != 0u) { tallies[P.flag] = 1u; }
        state[o] = v.x;
    } else { state[o] = v.x; state[o + 1u] = v.y; }
}
fn heat_at(copy: u32, c: u32, f: u32) -> I {
    let slot = c / 512u; let l = c % 512u;
    return st(copy * P.copy_words + P.heat_base + 2u * ((slot * 3u + f) * 512u + l));
}
fn tb(o: u32) -> I { return I(table[o], table[o + 1u]); }
fn gas(c: u32, k: u32) -> I {
    if (c == OUT) { return tb(P.outside + 2u * k); }
    return gas_at(P.in_copy, c, k);
}
fn lsd(o: u32) -> I { return I(sides_rw[o], sides_rw[o + 1u]); }
fn tsd(o: u32) -> I { return I(table[o], table[o + 1u]); }
fn side(c: u32) -> Side {
    var s: Side;
    if (c == OUT) {
        let o = P.outside + 20u;
        s.t = tsd(o); s.t_cond = tsd(o + 2u); s.t_skin = tsd(o + 4u); s.gas_ml = tsd(o + 6u);
        s.half = tsd(o + 8u); s.excess = tsd(o + 10u); s.flame_t = tsd(o + 12u); s.flame_heat = tsd(o + 14u);
        s.flame_fill = tsd(o + 16u); s.fuel_g = tsd(o + 18u); s.reached = tsd(o + 20u); s.water_g = tsd(o + 22u);
        let f = table[o + 24u];
        s.permeable = (f & 1u) != 0u; s.wet = (f & 2u) != 0u;
        return s;
    }
    let o = c * SS;
    s.t = lsd(o); s.t_cond = lsd(o + 2u); s.t_skin = lsd(o + 4u); s.gas_ml = lsd(o + 6u);
    s.half = lsd(o + 8u); s.excess = lsd(o + 10u); s.flame_t = lsd(o + 12u); s.flame_heat = lsd(o + 14u);
    s.flame_fill = lsd(o + 16u); s.fuel_g = lsd(o + 18u); s.reached = lsd(o + 20u); s.water_g = lsd(o + 22u);
    let f = sides_rw[o + 24u];
    s.permeable = (f & 1u) != 0u; s.wet = (f & 2u) != 0u;
    return s;
}

fn lxyz(l: u32) -> vec3<u32> { return vec3<u32>(l % 8u, l / 8u % 8u, l / 64u); }
fn lidx(p: vec3<u32>) -> u32 { return (p.z * 8u + p.y) * 8u + p.x; }

// The cell one step along `axis` (up: +1, else −1) from cell c: another
// cell, OUT (outside air) or NONE (a mirror: nothing crosses).
fn step(c: u32, axis: u32, up: bool, permeable: bool) -> u32 {
    let slot = c / 512u;
    var p = lxyz(c % 512u);
    var d = vec3<i32>(0);
    if (up) {
        if (p[axis] < 7u) { p[axis] += 1u; return slot * 512u + lidx(p); }
        p[axis] = 0u; d[axis] = 1;
    } else {
        if (p[axis] > 0u) { p[axis] -= 1u; return slot * 512u + lidx(p); }
        p[axis] = 7u; d[axis] = -1;
    }
    let n = table[slot * 26u + nslot(d)];
    if (n != OUT) { return n * 512u + lidx(p); }
    // The box's edge.
    if (axis == 2u) { return select(NONE, OUT, up); }
    return select(NONE, OUT, permeable);
}
// The 26-slot table's index for a step (dx, dy, dz), each −1, 0 or 1.
fn nslot(d: vec3<i32>) -> u32 {
    let k = u32((d.z + 1) * 9 + (d.y + 1) * 3 + (d.x + 1));
    return select(k, k - 1u, k > 13u);
}
// The velocity across the + face of cell c along axis.
fn vplus(c: u32, axis: u32) -> i32 { return vel[(c / 512u * 3u + axis) * 512u + c % 512u]; }
// The velocity across the − face of cell c, whose − neighbour is nb.
fn vminus(c: u32, axis: u32, nb: u32) -> i32 {
    if (nb != OUT && nb != NONE) { return vplus(nb, axis); }
    let p = lxyz(c % 512u);
    var j = 0u;
    if (axis == 0u) { j = p.y + 8u * p.z; } else if (axis == 1u) { j = p.x + 8u * p.z; } else { j = p.x + 8u * p.y; }
    return vel[P.vlow + (c / 512u * 3u + axis) * 64u + j];
}

// A face's flux, or none (a mirror): lower cell lo, upper cell hi.
struct Face { f: Flux, present: bool }
fn face(lo: u32, hi: u32, speed: i32, axis: u32) -> Face {
    var r: Face;
    r.present = lo != NONE && hi != NONE;
    if (!r.present) { r.f = zero_flux(); return r; }
    r.f = diffuse_face(lo, side(lo), hi, side(hi), abs_(i_(speed)), axis);
    return r;
}
fn zero_flux() -> Flux {
    var f: Flux;
    f.energy = I(0u, 0u); f.carried = I(0u, 0u); f.flame = I(0u, 0u); f.film = false;
    for (var k = 0u; k < 7u; k++) { f.gas[k] = I(0u, 0u); }
    return f;
}

// This thread's cell: workgroup w is tile w % 8 of slot w / 8.
fn my_cell(wg: vec3<u32>, t: u32) -> u32 {
    let w = wg.x + wg.y * P.tiles_x;
    let slot = w / 8u; let tile = w % 8u;
    let o = vec3<u32>(tile % 2u * 4u, tile / 2u % 2u * 4u, tile / 4u * 4u);
    return slot * 512u + lidx(o + vec3<u32>(t % 4u, t / 4u % 4u, t / 16u));
}
fn tile_pos(t: u32, axis: u32) -> u32 { return vec3<u32>(t % 4u, t / 4u % 4u, t / 16u)[axis]; }
// Slot in the workgroup's face arrays of a tile cell's lower boundary face:
// 64 + its index across the other two axes.
fn low_slot(t: u32, axis: u32) -> u32 {
    let p = vec3<u32>(t % 4u, t / 4u % 4u, t / 16u);
    if (axis == 0u) { return 64u + p.y + 4u * p.z; }
    if (axis == 1u) { return 64u + p.x + 4u * p.z; }
    return 64u + p.x + 4u * p.y;
}
fn stride(axis: u32) -> u32 { return select(select(16u, 4u, axis == 1u), 1u, axis == 0u); }

// ── pass 1: the limiters ─────────────────────────────────────────────────
var<workgroup> wgas: array<array<vec2<u32>, 7>, 80>;
var<workgroup> wpresent: array<u32, 80>;

const FULL: i32 = 1000000;

@compute @workgroup_size(64)
fn limits_pass(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_index) t: u32) {
    let w = wg.x + wg.y * P.tiles_x;
    let live = w < P.slots * 8u;
    var c = 0u;
    var perm = false;
    if (live) { c = my_cell(wg, t); perm = side(c).permeable; }
    var leaving: array<I, 7>;
    for (var k = 0u; k < 7u; k++) { leaving[k] = I(0u, 0u); }
    for (var axis = 0u; axis < 3u; axis++) {
        if (live) {
            let up = step(c, axis, true, perm);
            let f = face(c, up, vplus(c, axis), axis);
            for (var k = 0u; k < 7u; k++) { wgas[t][k] = f.f.gas[k]; }
            wpresent[t] = select(0u, 1u, f.present);
            if (tile_pos(t, axis) == 0u) {
                let dn = step(c, axis, false, perm);
                let g = face(dn, c, vminus(c, axis, dn), axis);
                let s = low_slot(t, axis);
                for (var k = 0u; k < 7u; k++) { wgas[s][k] = g.f.gas[k]; }
                wpresent[s] = select(0u, 1u, g.present);
            }
        }
        workgroupBarrier();
        if (live) {
            let dn_s = select(t - stride(axis), low_slot(t, axis), tile_pos(t, axis) == 0u);
            // + face: this cell is the lower one, it sends what flows up.
            if (wpresent[t] != 0u) {
                for (var k = 0u; k < 7u; k++) { let g = wgas[t][k]; if (gt0(g)) { leaving[k] = add(leaving[k], g); } }
            }
            // − face: it sends what flows down.
            if (wpresent[dn_s] != 0u) {
                for (var k = 0u; k < 7u; k++) { let g = wgas[dn_s][k]; if (isneg(g)) { leaving[k] = sub(leaving[k], g); } }
            }
        }
        workgroupBarrier();
    }
    if (!live) { return; }
    var ppm = i_(FULL);
    for (var k = 0u; k < 7u; k++) {
        let can = divi(muli(mx(gas(c, k), I(0u, 0u)), 7), 8);
        if (lt(can, leaving[k])) { ppm = mn(ppm, mul_div(can, i_(FULL), leaving[k])); }
    }
    limits[2u * c] = ppm.x; limits[2u * c + 1u] = ppm.y;
}

// ── pass 2: apply ────────────────────────────────────────────────────────
var<workgroup> wflux: array<array<vec2<u32>, 10>, 80>;
var<workgroup> wflags: array<u32, 80>;
var<workgroup> wtally: array<array<vec2<u32>, 8>, 64>;

fn put(s: u32, f: Face) {
    wflux[s][0] = f.f.energy; wflux[s][1] = f.f.carried; wflux[s][2] = f.f.flame;
    for (var k = 0u; k < 7u; k++) { wflux[s][3u + k] = f.f.gas[k]; }
    wflags[s] = select(0u, 1u, f.present) | select(0u, 2u, f.f.film);
}
fn take(s: u32) -> Flux {
    var f: Flux;
    f.energy = wflux[s][0]; f.carried = wflux[s][1]; f.flame = wflux[s][2];
    for (var k = 0u; k < 7u; k++) { f.gas[k] = wflux[s][3u + k]; }
    f.film = (wflags[s] & 2u) != 0u;
    return f;
}
fn limit(c: u32) -> I { return I(limits[2u * c], limits[2u * c + 1u]); }
fn scale_gas(f: ptr<function, Flux>, ppm: I) {
    if (!lt(ppm, i_(FULL))) { return; }
    for (var k = 0u; k < 7u; k++) { (*f).gas[k] = mul_div_1e6((*f).gas[k], ppm); }
    (*f).carried = mul_div_1e6((*f).carried, ppm);
}

@compute @workgroup_size(64)
fn apply_pass(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_index) t: u32) {
    let w = wg.x + wg.y * P.tiles_x;
    let live = w < P.slots * 8u;
    var c = 0u;
    var my: Side;
    if (live) { c = my_cell(wg, t); my = side(c); }
    var g: array<I, 7>;
    var energy = I(0u, 0u); var solid_e = I(0u, 0u); var skin = I(0u, 0u);
    var t_e = I(0u, 0u);
    var t_g: array<I, 7>;
    for (var k = 0u; k < 7u; k++) { t_g[k] = I(0u, 0u); g[k] = I(0u, 0u); }
    if (live) {
        for (var k = 0u; k < 7u; k++) { g[k] = gas(c, k); }
        energy = heat_at(P.in_copy, c, 0u); solid_e = heat_at(P.in_copy, c, 1u); skin = heat_at(P.in_copy, c, 2u);
    }
    for (var axis = 0u; axis < 3u; axis++) {
        var nbs = vec2<u32>(NONE, NONE);
        if (live) {
            let up = step(c, axis, true, my.permeable);
            let dn = step(c, axis, false, my.permeable);
            nbs = vec2<u32>(dn, up);
            put(t, face(c, up, vplus(c, axis), axis));
            if (tile_pos(t, axis) == 0u) { put(low_slot(t, axis), face(dn, c, vminus(c, axis, dn), axis)); }
        }
        workgroupBarrier();
        if (live) {
            for (var d = 0u; d < 2u; d++) {
                let isup = d == 1u;
                let s = select(select(t - stride(axis), low_slot(t, axis), tile_pos(t, axis) == 0u), t, isup);
                if ((wflags[s] & 1u) == 0u) { continue; }
                var f = take(s);
                let n = select(nbs.x, nbs.y, isup);
                let inside = n != OUT;
                if (inside) { scale_gas(&f, mn(limit(c), limit(n))); } else { scale_gas(&f, limit(c)); }
                let e = select(f.energy, neg_(f.energy), isup);
                if (my.wet) { solid_e = add(solid_e, e); }
                else if (my.permeable) { energy = add(energy, e); }
                else if (f.film) { skin = add(skin, e); }
                else { solid_e = add(solid_e, e); }
                let flame = select(f.flame, neg_(f.flame), isup);
                if (isneg(flame)) { energy = add(energy, flame); } else { skin = add(skin, flame); }
                let cr = select(f.carried, neg_(f.carried), isup);
                energy = add(energy, cr);
                for (var k = 0u; k < 7u; k++) { g[k] = add(g[k], select(f.gas[k], neg_(f.gas[k]), isup)); }
                if (!inside) {
                    t_e = add(t_e, add(e, cr));
                    for (var k = 0u; k < 7u; k++) { t_g[k] = add(t_g[k], select(f.gas[k], neg_(f.gas[k]), isup)); }
                }
            }
        }
        workgroupBarrier();
    }
    if (live && gt0(my.fuel_g) && lt(my.t, my.t_skin)) {
        var lean = 0;
        for (var axis = 0u; axis < 2u; axis++) {
            for (var d = 0u; d < 2u; d++) {
                // Only neighbours inside the box (a missing brick is its edge).
                let n = step(c, axis, d == 1u, false);
                if (n == OUT || n == NONE) { continue; }
                if (!iszero(lsd(n * SS + 12u))) { lean += C_FLAME_LEAN; }
            }
        }
        lean = min(lean, 1000);
        if (lean > 0) {
            let gg = divi(muli(my.fuel_g, lean), 1000);
            let e0 = heat_at(P.in_copy, c, 0u);
            let latent = muli(gas(c, C_VAPOUR), C_LATENT);
            let q = mn(heat(gg, sub(my.t_skin, my.t)), mx(divi(sub(e0, latent), 4), I(0u, 0u)));
            energy = sub(energy, q);
            skin = add(skin, q);
        }
    }
    if (live) {
        let slot = c / 512u; let l = c % 512u;
        let ob = P.out_copy * P.copy_words;
        for (var k = 0u; k < 7u; k++) { put_gas(P.out_copy, c, k, g[k]); }
        var hs = array<I, 3>(energy, solid_e, skin);
        for (var f = 0u; f < 3u; f++) { let o = ob + P.heat_base + 2u * ((slot * 3u + f) * 512u + l); state[o] = hs[f].x; state[o + 1u] = hs[f].y; }
    }
    // The tile's edge tally: an integer sum, in thread order.
    wtally[t][0] = t_e;
    for (var k = 0u; k < 7u; k++) { wtally[t][1u + k] = t_g[k]; }
    workgroupBarrier();
    if (t == 0u && live) {
        var s: array<I, 8>;
        for (var j = 0u; j < 8u; j++) { s[j] = I(0u, 0u); }
        for (var i = 0u; i < 64u; i++) { for (var j = 0u; j < 8u; j++) { s[j] = add(s[j], wtally[i][j]); } }
        for (var j = 0u; j < 8u; j++) { tallies[w * 16u + 2u * j] = s[j].x; tallies[w * 16u + 2u * j + 1u] = s[j].y; }
    }
}

// ── the same phase with a global face scratch ─────────────────────────────
// Every face worked out once a tick (not once per pass) and kept per brick:
// each cell's three + faces, then each brick's lower boundary faces where
// no brick lies below (3 × 64). Costs ~280 B per active cell of transient
// memory and saves half the face arithmetic, which §3 of the notes shows is
// the limit while 64-bit arithmetic is emulated.
@group(0) @binding(7) var<storage, read_write> faces: array<u32>;
const FS: u32 = 21u;
const FACES_PER_SLOT: u32 = 1728u;

fn plus_slot(c: u32, axis: u32) -> u32 { return c / 512u * FACES_PER_SLOT + axis * 512u + c % 512u; }
fn low_face(c: u32, axis: u32) -> u32 {
    let p = lxyz(c % 512u);
    var j = 0u;
    if (axis == 0u) { j = p.y + 8u * p.z; } else if (axis == 1u) { j = p.x + 8u * p.z; } else { j = p.x + 8u * p.y; }
    return c / 512u * FACES_PER_SLOT + 1536u + axis * 64u + j;
}
fn store_face(s: u32, f: Face) {
    let o = s * FS;
    faces[o] = f.f.energy.x; faces[o + 1u] = f.f.energy.y; faces[o + 2u] = f.f.carried.x; faces[o + 3u] = f.f.carried.y;
    for (var k = 0u; k < 7u; k++) { faces[o + 4u + 2u * k] = f.f.gas[k].x; faces[o + 5u + 2u * k] = f.f.gas[k].y; }
    faces[o + 18u] = f.f.flame.x; faces[o + 19u] = f.f.flame.y;
    faces[o + 20u] = select(0u, 1u, f.present) | select(0u, 2u, f.f.film);
}
fn fw(o: u32) -> I { return I(faces[o], faces[o + 1u]); }
fn load_face(s: u32) -> Flux {
    let o = s * FS;
    var f: Flux;
    f.energy = fw(o); f.carried = fw(o + 2u);
    for (var k = 0u; k < 7u; k++) { f.gas[k] = fw(o + 4u + 2u * k); }
    f.flame = fw(o + 18u); f.film = (faces[o + 20u] & 2u) != 0u;
    return f;
}
fn has_face(s: u32) -> bool { return (faces[s * FS + 20u] & 1u) != 0u; }
// The face slot on side `up` of cell c along axis, and the cell beyond.
fn face_slot(c: u32, axis: u32, up: bool, permeable: bool) -> vec2<u32> {
    let n = step(c, axis, up, permeable);
    if (up) { return vec2<u32>(plus_slot(c, axis), n); }
    if (n != OUT && n != NONE) { return vec2<u32>(plus_slot(n, axis), n); }
    return vec2<u32>(low_face(c, axis), n);
}
fn cell_of(id: vec3<u32>) -> u32 { return id.x + id.y * P.tiles_x * 64u; }

@compute @workgroup_size(64)
fn faces_scratch(@builtin(global_invocation_id) id: vec3<u32>) {
    let c = cell_of(id);
    if (c >= P.slots * 512u) { return; }
    let perm = side(c).permeable;
    let p = lxyz(c % 512u);
    for (var axis = 0u; axis < 3u; axis++) {
        store_face(plus_slot(c, axis), face(c, step(c, axis, true, perm), vplus(c, axis), axis));
        if (p[axis] == 0u) {
            let dn = step(c, axis, false, perm);
            if (dn == OUT || dn == NONE) { store_face(low_face(c, axis), face(dn, c, vminus(c, axis, dn), axis)); }
        }
    }
}

@compute @workgroup_size(64)
fn limits_scratch(@builtin(global_invocation_id) id: vec3<u32>) {
    let c = cell_of(id);
    if (c >= P.slots * 512u) { return; }
    let perm = side(c).permeable;
    var leaving: array<I, 7>;
    for (var k = 0u; k < 7u; k++) { leaving[k] = I(0u, 0u); }
    for (var axis = 0u; axis < 3u; axis++) {
        for (var d = 0u; d < 2u; d++) {
            let up = d == 1u;
            let fs = face_slot(c, axis, up, perm);
            if (!has_face(fs.x)) { continue; }
            for (var k = 0u; k < 7u; k++) {
                let g = fw(fs.x * FS + 4u + 2u * k);
                if (up && gt0(g)) { leaving[k] = add(leaving[k], g); }
                if (!up && isneg(g)) { leaving[k] = sub(leaving[k], g); }
            }
        }
    }
    var ppm = i_(FULL);
    for (var k = 0u; k < 7u; k++) {
        let can = divi(muli(mx(gas(c, k), I(0u, 0u)), 7), 8);
        if (lt(can, leaving[k])) { ppm = mn(ppm, mul_div(can, i_(FULL), leaving[k])); }
    }
    limits[2u * c] = ppm.x; limits[2u * c + 1u] = ppm.y;
}

@compute @workgroup_size(64)
fn apply_scratch(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_index) t: u32, @builtin(workgroup_id) wg: vec3<u32>) {
    let c = cell_of(id);
    let live = c < P.slots * 512u;
    var t_e = I(0u, 0u);
    var t_g: array<I, 7>;
    for (var k = 0u; k < 7u; k++) { t_g[k] = I(0u, 0u); }
    if (live) {
        let my = side(c);
        var g: array<I, 7>;
        for (var k = 0u; k < 7u; k++) { g[k] = gas(c, k); }
        var energy = heat_at(P.in_copy, c, 0u); var solid_e = heat_at(P.in_copy, c, 1u); var skin = heat_at(P.in_copy, c, 2u);
        for (var axis = 0u; axis < 3u; axis++) {
            for (var d = 0u; d < 2u; d++) {
                let isup = d == 1u;
                let fs = face_slot(c, axis, isup, my.permeable);
                if (!has_face(fs.x)) { continue; }
                var f = load_face(fs.x);
                let inside = fs.y != OUT;
                if (inside) { scale_gas(&f, mn(limit(c), limit(fs.y))); } else { scale_gas(&f, limit(c)); }
                let e = select(f.energy, neg_(f.energy), isup);
                if (my.wet) { solid_e = add(solid_e, e); }
                else if (my.permeable) { energy = add(energy, e); }
                else if (f.film) { skin = add(skin, e); }
                else { solid_e = add(solid_e, e); }
                let flame = select(f.flame, neg_(f.flame), isup);
                if (isneg(flame)) { energy = add(energy, flame); } else { skin = add(skin, flame); }
                let cr = select(f.carried, neg_(f.carried), isup);
                energy = add(energy, cr);
                for (var k = 0u; k < 7u; k++) { g[k] = add(g[k], select(f.gas[k], neg_(f.gas[k]), isup)); }
                if (!inside) {
                    t_e = add(t_e, add(e, cr));
                    for (var k = 0u; k < 7u; k++) { t_g[k] = add(t_g[k], select(f.gas[k], neg_(f.gas[k]), isup)); }
                }
            }
        }
        if (gt0(my.fuel_g) && lt(my.t, my.t_skin)) {
            var lean = 0;
            for (var axis = 0u; axis < 2u; axis++) {
                for (var d = 0u; d < 2u; d++) {
                    let n = step(c, axis, d == 1u, false);
                    if (n == OUT || n == NONE) { continue; }
                    if (!iszero(lsd(n * SS + 12u))) { lean += C_FLAME_LEAN; }
                }
            }
            lean = min(lean, 1000);
            if (lean > 0) {
                let gg = divi(muli(my.fuel_g, lean), 1000);
                let e0 = heat_at(P.in_copy, c, 0u);
                let latent = muli(gas(c, C_VAPOUR), C_LATENT);
                let q = mn(heat(gg, sub(my.t_skin, my.t)), mx(divi(sub(e0, latent), 4), I(0u, 0u)));
                energy = sub(energy, q);
                skin = add(skin, q);
            }
        }
        for (var k = 0u; k < 7u; k++) { put_gas(P.out_copy, c, k, g[k]); }
        let slot = c / 512u; let l = c % 512u;
        let ob = P.out_copy * P.copy_words + P.heat_base;
        var hs = array<I, 3>(energy, solid_e, skin);
        for (var f = 0u; f < 3u; f++) { let o = ob + 2u * ((slot * 3u + f) * 512u + l); state[o] = hs[f].x; state[o + 1u] = hs[f].y; }
    }
    // The workgroup's 64 cells (an eighth of a brick, in cell order): its
    // edge tally, an integer sum in thread order.
    wtally[t][0] = t_e;
    for (var k = 0u; k < 7u; k++) { wtally[t][1u + k] = t_g[k]; }
    workgroupBarrier();
    let w = wg.x + wg.y * P.tiles_x;
    if (t == 0u && w < P.slots * 8u) {
        var s: array<I, 8>;
        for (var j = 0u; j < 8u; j++) { s[j] = I(0u, 0u); }
        for (var i = 0u; i < 64u; i++) { for (var j = 0u; j < 8u; j++) { s[j] = add(s[j], wtally[i][j]); } }
        for (var j = 0u; j < 8u; j++) { tallies[w * 16u + 2u * j] = s[j].x; tallies[w * 16u + 2u * j + 1u] = s[j].y; }
    }
}

// ── carrying (transport::advect), one sub-step ─────────────────────────────
// Air moving through a face takes a share of the donor's gas, and the heat
// it holds, across it. Only cells air passes through take part (the
// permeable flag the CPU worked out with the sides: carrying changes no
// solid). Every share is from the sub-step's starting state.
// `limits` holds each cell's gas temperature for the sub-step, and the first
// words of `faces` each cell's total outflow share (both free during carrying).

const ONE_HI: u32 = 1u; // Fraction::ONE = 2³² is I(0, 1)

fn gas_temperature(c: u32) -> I {
    var cap = I(0u, 0u);
    for (var k = 0u; k < 7u; k++) { cap = add(cap, muli(gas(c, k), C_GAS_SH[k])); }
    if (!gt0(cap)) { return I(0u, 0u); }
    let e = sub(heat_at(P.in_copy, c, 0u), muli(gas(c, C_VAPOUR), C_LATENT));
    return mul_div(e, i_(1000000), cap);
}
// |u|·dt/dx as a share of 2³²: u × 50 ms × 2³² / (100 mm × 1000 × steps),
// the divisor as 400 × 250 × steps (all below 2¹⁶; nested floors are exact).
fn share_of_speed(u: u32) -> I {
    let n = I(0u, u * u32(C_TICK));
    return udiv64_small(udiv64_small(udiv64_small(n, 400u), 250u), P.steps);
}
fn outflow_total(c: u32) -> I {
    var t = I(0u, 0u);
    for (var axis = 0u; axis < 3u; axis++) {
        let plus = vplus(c, axis);
        let dn = step(c, axis, false, true);
        let minus = vminus(c, axis, dn);
        if (plus > 0) { t = add(t, share_of_speed(u32(plus))); }
        if (minus < 0) { t = add(t, share_of_speed(u32(-minus))); }
    }
    return t;
}
fn face_share(total: I, u: i32) -> I {
    let s = share_of_speed(u32(abs(u)));
    if (!ult(I(0u, ONE_HI), total)) { return s; }
    return udivw(W(0u, s.x, s.y, 0u), W(total.x, total.y, 0u, 0u)).xy;
}
fn temp_of(c: u32) -> I {
    if (c == OUT) { return I(P.outside_t_lo, P.outside_t_hi); }
    return I(limits[2u * c], limits[2u * c + 1u]);
}
fn total_of(c: u32) -> I { return I(faces[2u * c], faces[2u * c + 1u]); }
fn permeable_of(c: u32) -> bool { return (sides_rw[c * SS + 24u] & 1u) != 0u; }

@compute @workgroup_size(64)
fn carry_prepare(@builtin(global_invocation_id) id: vec3<u32>) {
    let c = cell_of(id);
    if (c >= P.slots * 512u) { return; }
    let t = gas_temperature(c);
    limits[2u * c] = t.x; limits[2u * c + 1u] = t.y;
    let tot = outflow_total(c);
    faces[2u * c] = tot.x; faces[2u * c + 1u] = tot.y;
}

// share × a mass, rounding down (Fraction::of_mass: (m × share) >> 32).
fn of_mass(m: I, share: I) -> I { let w = mulw(m, share); return I(w.y, w.z); }

@compute @workgroup_size(64)
fn carry_apply(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_index) t: u32, @builtin(workgroup_id) wg: vec3<u32>) {
    let c = cell_of(id);
    let live = c < P.slots * 512u;
    var t_e = I(0u, 0u);
    var t_g: array<I, 7>;
    for (var k = 0u; k < 7u; k++) { t_g[k] = I(0u, 0u); }
    if (live) {
        var g: array<I, 7>;
        for (var k = 0u; k < 7u; k++) { g[k] = gas(c, k); }
        var energy = heat_at(P.in_copy, c, 0u);
        if (permeable_of(c)) {
            for (var axis = 0u; axis < 3u; axis++) {
                for (var d = 0u; d < 2u; d++) {
                    let up = d == 1u;
                    let n = step(c, axis, up, true);
                    let u = select(vminus(c, axis, n), vplus(c, axis), up);
                    if (u == 0 || n == NONE) { continue; }
                    // The donor, its share, and the sign of the flux (+1: lower to upper).
                    var donor = c; var share = I(0u, 0u); var sign_up = true;
                    if (n != OUT) {
                        if (!permeable_of(n)) { continue; }
                        let lo = select(n, c, up); let hi = select(c, n, up);
                        donor = select(hi, lo, u > 0);
                        sign_up = u > 0;
                        share = face_share(total_of(donor), u);
                    } else {
                        let leaving = (u > 0) == up;
                        if (leaving) { donor = c; share = face_share(total_of(c), u); sign_up = up; }
                        else { donor = OUT; share = mn(share_of_speed(u32(abs(u))), I(0u, ONE_HI)); sign_up = !up; }
                    }
                    let td = temp_of(donor);
                    // Applied with sign −dir: the lower cell gives what flows up.
                    let gives = sign_up == up;
                    for (var k = 0u; k < 7u; k++) {
                        let m = of_mass(gas(donor, k), share);
                        let e = carried(k, m, td);
                        if (gives) { g[k] = sub(g[k], m); energy = sub(energy, e); } else { g[k] = add(g[k], m); energy = add(energy, e); }
                        if (n == OUT) {
                            if (gives) { t_g[k] = sub(t_g[k], m); t_e = sub(t_e, e); } else { t_g[k] = add(t_g[k], m); t_e = add(t_e, e); }
                        }
                    }
                }
            }
        }
        for (var k = 0u; k < 7u; k++) { put_gas(P.out_copy, c, k, g[k]); }
        let slot = c / 512u; let l = c % 512u;
        let ob = P.out_copy * P.copy_words + P.heat_base;
        let ib = P.in_copy * P.copy_words + P.heat_base;
        for (var f = 0u; f < 3u; f++) {
            let o = 2u * ((slot * 3u + f) * 512u + l);
            if (f == 0u) { state[ob + o] = energy.x; state[ob + o + 1u] = energy.y; }
            else { state[ob + o] = state[ib + o]; state[ob + o + 1u] = state[ib + o + 1u]; }
        }
    }
    wtally[t][0] = t_e;
    for (var k = 0u; k < 7u; k++) { wtally[t][1u + k] = t_g[k]; }
    workgroupBarrier();
    let w = wg.x + wg.y * P.tiles_x;
    if (t == 0u && w < P.slots * 8u) {
        var s: array<I, 8>;
        for (var j = 0u; j < 8u; j++) { s[j] = I(0u, 0u); }
        if (P.accumulate != 0u) { for (var j = 0u; j < 8u; j++) { s[j] = I(tallies[w * 16u + 2u * j], tallies[w * 16u + 2u * j + 1u]); } }
        for (var i = 0u; i < 64u; i++) { for (var j = 0u; j < 8u; j++) { s[j] = add(s[j], wtally[i][j]); } }
        for (var j = 0u; j < 8u; j++) { tallies[w * 16u + 2u * j] = s[j].x; tallies[w * 16u + 2u * j + 1u] = s[j].y; }
    }
}

// ── carrying with each face worked out once ─────────────────────────────────
// The same sub-step, in two passes: every face's carried flux into the face
// scratch (16 words: 7 gases and the heat they hold, + from the lower cell
// to the upper; zero where nothing is carried), then each cell adds its six.
// Each face is a pure function of the sub-step's starting state, so this is
// exactly what each side would compute for itself. The outflow totals sit
// past the faces in the same scratch.
const CF: u32 = 16u;
fn total_slot(c: u32) -> u32 { return P.slots * FACES_PER_SLOT * CF + 2u * c; }
fn total2(c: u32) -> I { let o = total_slot(c); return I(faces[o], faces[o + 1u]); }

@compute @workgroup_size(64)
fn carry_prepare2(@builtin(global_invocation_id) id: vec3<u32>) {
    let c = cell_of(id);
    if (c >= P.slots * 512u) { return; }
    let t = gas_temperature(c);
    limits[2u * c] = t.x; limits[2u * c + 1u] = t.y;
    let tot = outflow_total(c);
    let o = total_slot(c);
    faces[o] = tot.x; faces[o + 1u] = tot.y;
}

// The carried flux across the face between lower cell lo and upper cell hi
// (either may be OUT), u the velocity across it; + is lower to upper.
fn carry_face(lo: u32, hi: u32, u: i32, s: u32) {
    let o = s * CF;
    var gas_f: array<I, 7>;
    var heat_f = I(0u, 0u);
    for (var k = 0u; k < 7u; k++) { gas_f[k] = I(0u, 0u); }
    let inside = lo != OUT && hi != OUT;
    var ok = u != 0 && lo != NONE && hi != NONE;
    if (ok) {
        // Only between cells air passes through (the outside always does).
        if (lo != OUT && !permeable_of(lo)) { ok = false; }
        if (hi != OUT && !permeable_of(hi)) { ok = false; }
    }
    if (ok) {
        let donor = select(hi, lo, u > 0);
        var share = I(0u, 0u);
        if (donor == OUT) { share = mn(share_of_speed(u32(abs(u))), I(0u, ONE_HI)); }
        else { share = face_share(total2(donor), u); }
        let td = temp_of(donor);
        for (var k = 0u; k < 7u; k++) {
            let m = of_mass(gas(donor, k), share);
            let e = carried(k, m, td);
            if (u > 0) { gas_f[k] = m; heat_f = add(heat_f, e); } else { gas_f[k] = neg_(m); heat_f = sub(heat_f, e); }
        }
    }
    for (var k = 0u; k < 7u; k++) { faces[o + 2u * k] = gas_f[k].x; faces[o + 2u * k + 1u] = gas_f[k].y; }
    faces[o + 14u] = heat_f.x; faces[o + 15u] = heat_f.y;
}

@compute @workgroup_size(64)
fn carry_faces(@builtin(global_invocation_id) id: vec3<u32>) {
    let c = cell_of(id);
    if (c >= P.slots * 512u) { return; }
    let p = lxyz(c % 512u);
    for (var axis = 0u; axis < 3u; axis++) {
        let up = step(c, axis, true, true);
        carry_face(c, up, vplus(c, axis), plus_slot(c, axis));
        if (p[axis] == 0u) {
            let dn = step(c, axis, false, true);
            if (dn == OUT || dn == NONE) { carry_face(dn, c, vminus(c, axis, dn), low_face(c, axis)); }
        }
    }
}

@compute @workgroup_size(64)
fn carry_apply2(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_index) t: u32, @builtin(workgroup_id) wg: vec3<u32>) {
    let c = cell_of(id);
    let live = c < P.slots * 512u;
    var t_e = I(0u, 0u);
    var t_g: array<I, 7>;
    for (var k = 0u; k < 7u; k++) { t_g[k] = I(0u, 0u); }
    if (live) {
        var g: array<I, 7>;
        for (var k = 0u; k < 7u; k++) { g[k] = gas(c, k); }
        var energy = heat_at(P.in_copy, c, 0u);
        if (permeable_of(c)) {
            for (var axis = 0u; axis < 3u; axis++) {
                for (var d = 0u; d < 2u; d++) {
                    let up = d == 1u;
                    let fs = face_slot(c, axis, up, true);
                    let o = fs.x * CF;
                    // This cell is the lower one on its + face (it gives what flows up).
                    for (var k = 0u; k < 7u; k++) {
                        let m = I(faces[o + 2u * k], faces[o + 2u * k + 1u]);
                        g[k] = select(add(g[k], m), sub(g[k], m), up);
                        if (fs.y == OUT) { t_g[k] = select(add(t_g[k], m), sub(t_g[k], m), up); }
                    }
                    let e = I(faces[o + 14u], faces[o + 15u]);
                    energy = select(add(energy, e), sub(energy, e), up);
                    if (fs.y == OUT) { t_e = select(add(t_e, e), sub(t_e, e), up); }
                }
            }
        }
        for (var k = 0u; k < 7u; k++) { put_gas(P.out_copy, c, k, g[k]); }
        let slot = c / 512u; let l = c % 512u;
        let ob = P.out_copy * P.copy_words + P.heat_base;
        let ib = P.in_copy * P.copy_words + P.heat_base;
        for (var f = 0u; f < 3u; f++) {
            let o = 2u * ((slot * 3u + f) * 512u + l);
            if (f == 0u) { state[ob + o] = energy.x; state[ob + o + 1u] = energy.y; }
            else { state[ob + o] = state[ib + o]; state[ob + o + 1u] = state[ib + o + 1u]; }
        }
    }
    wtally[t][0] = t_e;
    for (var k = 0u; k < 7u; k++) { wtally[t][1u + k] = t_g[k]; }
    workgroupBarrier();
    let w = wg.x + wg.y * P.tiles_x;
    if (t == 0u && w < P.slots * 8u) {
        var s: array<I, 8>;
        for (var j = 0u; j < 8u; j++) { s[j] = I(0u, 0u); }
        if (P.accumulate != 0u) { for (var j = 0u; j < 8u; j++) { s[j] = I(tallies[w * 16u + 2u * j], tallies[w * 16u + 2u * j + 1u]); } }
        for (var i = 0u; i < 64u; i++) { for (var j = 0u; j < 8u; j++) { s[j] = add(s[j], wtally[i][j]); } }
        for (var j = 0u; j < 8u; j++) { tallies[w * 16u + 2u * j] = s[j].x; tallies[w * 16u + 2u * j + 1u] = s[j].y; }
    }
}
