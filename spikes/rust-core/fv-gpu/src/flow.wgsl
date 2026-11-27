// The flow (flow::Flow::step) on the GPU: buoyancy, wind, drag and a
// pressure solve on 2×2×2-cell blocks, one thread per block, every loop
// reading only the previous arrays (as on the CPU), the same integers.
// The blocks form a dense grid (cx × cy × cz); their cells are read from the
// pool through `aux`'s brick → slot map. The block arrays live in the face
// scratch (`faces`), which is free between diffusion and carrying.

struct FlowParams {
    cx: u32, cy: u32, cz: u32, nb: u32,
    wind_x: i32, wind_y: i32, bx: u32, by: u32,
    seed_lo: u32, seed_hi: u32, tick3_lo: u32, tick3_hi: u32,
    map: u32, p_in: u32, pad0: u32, pad1: u32,
}
@group(0) @binding(10) var<uniform> FP: FlowParams;

const FR: u32 = 2u;
const KEEP: i32 = 245;
const G_MM: i32 = 9807;
const MAX_SPEED: i32 = 3800;
const Q: i32 = 1024;

// Block arrays in `faces` (words): velocities in (A) and out (B), 3 per
// block; temperature, over-volume, drag and divergence, i64; open; give
// (3 × i64); pressure ping-pong.
fn o_vel_a() -> u32 { return 0u; }
fn o_vel_b() -> u32 { return 3u * FP.nb; }
fn o_temp() -> u32 { return 6u * FP.nb; }
fn o_over() -> u32 { return 8u * FP.nb; }
fn o_drag() -> u32 { return 10u * FP.nb; }
fn o_div() -> u32 { return 12u * FP.nb; }
fn o_open() -> u32 { return 14u * FP.nb; }
fn o_give() -> u32 { return 15u * FP.nb; }
fn o_p(k: u32) -> u32 { return 21u * FP.nb + 2u * k * FP.nb; }

fn fget(o: u32) -> I { return I(faces[o], faces[o + 1u]); }
fn fput(o: u32, v: I) { faces[o] = v.x; faces[o + 1u] = v.y; }
fn vel_at(base: u32, i: u32, a: u32) -> i32 { return bitcast<i32>(faces[base + 3u * i + a]); }
fn ci(x: u32, y: u32, z: u32) -> u32 { return (z * FP.cy + y) * FP.cx + x; }
fn bxyz(i: u32) -> vec3<u32> { return vec3<u32>(i % FP.cx, i / FP.cx % FP.cy, i / (FP.cx * FP.cy)); }
fn dims_b() -> vec3<i32> { return vec3<i32>(i32(FP.cx), i32(FP.cy), i32(FP.cz)); }

// Flow::block_face on the velocities at `base`.
fn block_face(base: u32, p: vec3<i32>, axis: u32) -> i32 {
    let n = dims_b();
    if (p[axis] < 0 || p[axis] >= n[axis] - 1) {
        if (axis == 2u) {
            if (p[axis] < 0) { return 0; }
            return vel_at(base, ci(u32(p.x), u32(p.y), u32(p.z)), 2u);
        }
        return select(FP.wind_y, FP.wind_x, axis == 0u);
    }
    return vel_at(base, ci(u32(p.x), u32(p.y), u32(p.z)), axis);
}

// The pool cell at world coordinates (x, y, z).
fn pool_cell(x: u32, y: u32, z: u32) -> u32 {
    let b = ((z / 8u) * FP.by + y / 8u) * FP.bx + x / 8u;
    return aux[FP.map + b] * 512u + ((z % 8u) * 8u + y % 8u) * 8u + x % 8u;
}

// rng::mix and rng::hash (splitmix64), in emulated 64 bits.
fn shr64(a: I, s: u32) -> I { return I((a.x >> s) | (a.y << (32u - s)), a.y >> s); }
fn xor64(a: I, b: I) -> I { return I(a.x ^ b.x, a.y ^ b.y); }
fn mix64(z0: I) -> I {
    var z = mul(xor64(z0, shr64(z0, 30u)), I(0x1ce4e5b9u, 0xbf58476du));
    z = mul(xor64(z, shr64(z, 27u)), I(0x133111ebu, 0x94d049bbu));
    return xor64(z, shr64(z, 31u));
}
fn hash4(a: I, b: I, c: I, d: I) -> I {
    return mix64(add(mix64(add(mix64(add(mix64(xor64(a, I(0x7f4a7c15u, 0x9e3779b9u))), b)), c)), d));
}
// u64 % m for 0 < m < 2¹⁶.
fn umod_small(a: I, m: u32) -> u32 { let q = udiv64_small(a, m); return a.x - q.x * m; }

// Floor division by a positive d (div_euclid).
fn floor_div(v: I, d: I) -> I {
    let q = div(v, d);
    if (isneg(v) && !eq(mul(q, d), v)) { return sub(q, i_(1)); }
    return q;
}

// flow::bed_drag.
fn bed_drag(x: C, s: S) -> I {
    if (!permeable(x, s) || iszero(s.sigma)) { return I(0u, 0u); }
    let beta = solid_share(x, s);
    return narrow(wdiv(mulw(mul(s.sigma, beta), i_(1000)), W(0u, 8u, 0u, 0u)));
}
// flow::give: 1/(1 + c·|u|·dt) permille.
fn give(c: I, u: i32) -> I {
    if (!gt0(c)) { return i_(1000); }
    let x = wdiv(mulw(c, i_(abs(u) * C_TICK)), widen(i_(2)));
    return narrow(wdiv(W(0xd4a51000u, 0xe8u, 0u, 0u), wadd(widen(i_(1000000000)), x))); // 10¹² / (10⁹ + x)
}

@compute @workgroup_size(64)
fn flow_gather(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x + id.y * 65535u * 64u;
    if (i >= FP.nb) { return; }
    let b = bxyz(i);
    var sum = I(0u, 0u); var cnt = 0; var over = I(0u, 0u); var dr = I(0u, 0u);
    let fresh = i_(C_FRESH_TOTAL);
    let amb = ambient();
    for (var d = 0u; d < 8u; d++) {
        let c = pool_cell(b.x * FR + (d & 1u), b.y * FR + ((d >> 1u) & 1u), b.z * FR + (d >> 2u));
        let x = load(c);
        let s = subst(x.substance);
        if (!permeable(x, s)) { continue; }
        dr = add(dr, bed_drag(x, s));
        let t = mx(gas_temp(x), i_(1));
        sum = add(sum, t);
        cnt += 1;
        let ml = gas_volume_ml(x, s);
        let fits = narrow(wdiv(wmul_small(mulw(fresh, ml), amb), mulw(i_(C_CELL_VOLUME_ML), t)));
        var gm = I(0u, 0u);
        for (var k = 0u; k < 7u; k++) { gm = add(gm, x.gas[k]); }
        over = add(over, div(muli(sub(gm, fits), C_CELL_VOLUME_ML), mx(fresh, i_(1))));
    }
    var temp = amb;
    if (cnt > 0) { temp = div(sum, i_(cnt)); }
    fput(o_temp() + 2u * i, temp);
    faces[o_open() + i] = select(0u, 1u, cnt >= 4);
    fput(o_over() + 2u * i, over);
    fput(o_drag() + 2u * i, divi(dr, 8));
}

// Flow::carried: each face's velocity is what arrives there from upstream
// (semi-Lagrangian), from velocities A into B.
fn at_face(p: vec3<i32>, a: u32) -> i32 {
    let n = dims_b();
    var q = p;
    for (var k = 0u; k < 3u; k++) {
        if (k == a) { q[k] = clamp(q[k], -1, n[k] - 1); } else { q[k] = clamp(q[k], 0, n[k] - 1); }
    }
    return block_face(o_vel_a(), q, a);
}
@compute @workgroup_size(64)
fn flow_carried(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x + id.y * 65535u * 64u;
    if (i >= FP.nb) { return; }
    let p = vec3<i32>(bxyz(i));
    let block_mm = i32(FR) * C_CELL_MM;
    for (var a = 0u; a < 3u; a++) {
        var u = vec3<i32>(0);
        for (var b = 0u; b < 3u; b++) {
            if (b == a) { u[b] = at_face(p, a); continue; }
            var s = 0;
            for (var k = 0u; k < 4u; k++) {
                var q = p;
                q[a] += i32(k >> 1u);
                q[b] -= i32(k & 1u);
                s += at_face(q, b);
            }
            u[b] = s / 4;
        }
        var f = vec3<i32>(0);
        for (var k = 0u; k < 3u; k++) { f[k] = p[k] * Q - u[k] * C_TICK * Q / (1000 * block_mm); }
        var base = vec3<i32>(0); var frac = vec3<i32>(0);
        for (var k = 0u; k < 3u; k++) {
            base[k] = f[k] >> 10u;          // floor division by 1024 (arithmetic shift)
            frac[k] = f[k] - base[k] * Q;   // rem_euclid
        }
        var v = I(0u, 0u);
        for (var corner = 0u; corner < 8u; corner++) {
            var q = base;
            var wgt = 1;
            for (var k = 0u; k < 3u; k++) {
                let hi = ((corner >> k) & 1u) == 1u;
                if (hi) { q[k] += 1; }
                wgt *= select(Q - frac[k], frac[k], hi);
            }
            v = add(v, mul(i_(wgt), i_(at_face(q, a))));
        }
        faces[o_vel_b() + 3u * i + a] = bitcast<u32>(i32(floor_div(v, i_(Q * Q * Q)).x));
    }
}

// The forces: damping, buoyancy, flicker in hot air, drag. From B into A.
@compute @workgroup_size(64)
fn flow_forced(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x + id.y * 65535u * 64u;
    if (i >= FP.nb) { return; }
    let b = bxyz(i);
    let amb = ambient();
    let n = vec3<u32>(FP.cx, FP.cy, FP.cz);
    for (var axis = 0u; axis < 3u; axis++) {
        var v = divi(muli(i_(vel_at(o_vel_b(), i, axis)), KEEP), 256);
        var q = b; q[axis] += 1u;
        let up_ok = q[axis] < n[axis];
        let j = select(i, ci(q.x, q.y, q.z), up_ok);
        var k = i_(1000);
        var out = 0;
        if (faces[o_open() + i] != 0u && up_ok && faces[o_open() + j] != 0u) {
            let t = divi(add(fget(o_temp() + 2u * i), fget(o_temp() + 2u * j)), 2);
            if (axis == 2u) { v = add(v, divi(muli(div(muli(sub(t, amb), G_MM), amb), C_TICK), 1000)); }
            let hot = divi(mx(sub(t, amb), I(0u, 0u)), 1000);
            if (lt(i_(20), hot)) {
                let h = hash4(I(FP.seed_lo, FP.seed_hi), I(FP.tick3_lo, FP.tick3_hi), I(i, 0u), I(axis, 0u));
                v = add(v, divi(muli(mn(hot, i_(600)), i32(umod_small(h, 201u)) - 100), 100));
            }
            v = mn(mx(v, i_(-MAX_SPEED)), i_(MAX_SPEED));
            k = give(add(fget(o_drag() + 2u * i), fget(o_drag() + 2u * j)), i32(v.x));
            out = i32(divi(mul(v, k), 1000).x);
        }
        faces[o_vel_a() + 3u * i + axis] = bitcast<u32>(out);
        fput(o_give() + 6u * i + 2u * axis, k);
    }
}

@compute @workgroup_size(64)
fn flow_div(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x + id.y * 65535u * 64u;
    if (i >= FP.nb) { return; }
    fput(o_p(0u) + 2u * i, I(0u, 0u));
    if (faces[o_open() + i] == 0u) { fput(o_div() + 2u * i, I(0u, 0u)); return; }
    let p = vec3<i32>(bxyz(i));
    let base = o_vel_a();
    let d = block_face(base, p, 0u) - block_face(base, p - vec3<i32>(1, 0, 0), 0u)
        + block_face(base, p, 1u) - block_face(base, p - vec3<i32>(0, 1, 0), 1u)
        + block_face(base, p, 2u) - block_face(base, p - vec3<i32>(0, 0, 1), 2u);
    // d − over × 10⁶ / (2 × dt × block face)
    let push = div(muli(fget(o_over() + 2u * i), 1000000), i_(2 * C_TICK * 4 * C_FACE_MM2));
    fput(o_div() + 2u * i, sub(i_(d), push));
}

@compute @workgroup_size(64)
fn flow_pressure(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x + id.y * 65535u * 64u;
    if (i >= FP.nb) { return; }
    let pin = o_p(FP.p_in); let pout = o_p(1u - FP.p_in);
    if (faces[o_open() + i] == 0u) { fput(pout + 2u * i, I(0u, 0u)); return; }
    let b = vec3<i32>(bxyz(i));
    let n = dims_b();
    var sum = I(0u, 0u); var cnt = I(0u, 0u);
    for (var f = 0u; f < 6u; f++) {
        let axis = f / 2u;
        let dir = select(-1, 1, (f & 1u) == 1u);
        var q = b; q[axis] += dir;
        let inside = all(q >= vec3<i32>(0)) && all(q < n);
        if (!inside) {
            if (q.z < 0) { continue; }
            cnt = add(cnt, i_(1000));
            continue;
        }
        let j = ci(u32(q.x), u32(q.y), u32(q.z));
        if (faces[o_open() + j] == 0u) { continue; }
        let k = select(fget(o_give() + 6u * j + 2u * axis), fget(o_give() + 6u * i + 2u * axis), dir > 0);
        sum = add(sum, mul(fget(pin + 2u * j), k));
        cnt = add(cnt, k);
    }
    var r = I(0u, 0u);
    if (gt0(cnt)) { r = div(sub(sum, muli(fget(o_div() + 2u * i), 1000)), cnt); }
    fput(pout + 2u * i, r);
}

// Subtract the pressure gradient: the tick's velocities, into B.
@compute @workgroup_size(64)
fn flow_project(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x + id.y * 65535u * 64u;
    if (i >= FP.nb) { return; }
    let b = bxyz(i);
    let n = vec3<u32>(FP.cx, FP.cy, FP.cz);
    let pp = o_p(FP.p_in);
    let open_i = faces[o_open() + i] != 0u;
    for (var axis = 0u; axis < 3u; axis++) {
        var v = vel_at(o_vel_a(), i, axis);
        var r = b; r[axis] += 1u;
        if (r[axis] >= n[axis]) {
            if (axis == 2u && open_i) { v = i32(mn(mx(add(i_(v), fget(pp + 2u * i)), i_(-MAX_SPEED)), i_(MAX_SPEED)).x); }
        } else {
            let j = ci(r.x, r.y, r.z);
            if (!(open_i && faces[o_open() + j] != 0u)) { v = 0; }
            else {
                let dv = div(mul(sub(fget(pp + 2u * j), fget(pp + 2u * i)), fget(o_give() + 6u * i + 2u * axis)), i_(1000));
                v = i32(mn(mx(sub(i_(v), dv), i_(-MAX_SPEED)), i_(MAX_SPEED)).x);
            }
        }
        faces[o_vel_b() + 3u * i + axis] = bitcast<u32>(v);
    }
}
