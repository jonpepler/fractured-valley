// The diffusion phase of fv-core (transport.rs: diffuse_face, gas_limits,
// diffuse_brick) as integer compute shaders, bit for bit.
//
// WGSL has no 64-bit integers, so an i64 is a vec2<u32> (x: low word, y:
// high word, two's complement) and an i128 is a vec4<u32> (x lowest). Every
// operation below gives exactly the bits Rust's i64/i128 give, wherever the
// CPU code doesn't overflow (where it would, the CPU panics). u32 arithmetic
// in WGSL is exactly specified (wrapping), so these are the same on every
// GPU.
//
// The constants (C_*) are prepended by the host from fv-core's own values.

alias I = vec2<u32>;
alias W = vec4<u32>;

const SIGN: u32 = 0x80000000u;

fn i_(v: i32) -> I { return I(bitcast<u32>(v), select(0u, 0xffffffffu, v < 0)); }
fn neg_(a: I) -> I { let lo = ~a.x + 1u; return I(lo, ~a.y + select(0u, 1u, lo == 0u)); }
fn add(a: I, b: I) -> I { let lo = a.x + b.x; return I(lo, a.y + b.y + select(0u, 1u, lo < a.x)); }
fn sub(a: I, b: I) -> I { return I(a.x - b.x, a.y - b.y - select(0u, 1u, a.x < b.x)); }
fn isneg(a: I) -> bool { return (a.y & SIGN) != 0u; }
fn iszero(a: I) -> bool { return (a.x | a.y) == 0u; }
fn eq(a: I, b: I) -> bool { return a.x == b.x && a.y == b.y; }
fn lt(a: I, b: I) -> bool {
    if (a.y != b.y) { return bitcast<i32>(a.y) < bitcast<i32>(b.y); }
    return a.x < b.x;
}
fn le(a: I, b: I) -> bool { return !lt(b, a); }
fn gt0(a: I) -> bool { return !isneg(a) && !iszero(a); }
fn mn(a: I, b: I) -> I { return select(b, a, lt(a, b)); }
fn mx(a: I, b: I) -> I { return select(b, a, lt(b, a)); }
fn abs_(a: I) -> I { return select(a, neg_(a), isneg(a)); }
fn ult(a: I, b: I) -> bool { return a.y < b.y || (a.y == b.y && a.x < b.x); }

// 32 × 32 → 64, unsigned, through 16-bit halves.
fn mul32w(a: u32, b: u32) -> I {
    let a0 = a & 0xffffu; let a1 = a >> 16u; let b0 = b & 0xffffu; let b1 = b >> 16u;
    let p00 = a0 * b0; let p01 = a0 * b1; let p10 = a1 * b0; let p11 = a1 * b1;
    let mid = (p00 >> 16u) + (p01 & 0xffffu) + (p10 & 0xffffu);
    return I((p00 & 0xffffu) | (mid << 16u), p11 + (p01 >> 16u) + (p10 >> 16u) + (mid >> 16u));
}
// The low 64 bits of a product: i64 × i64 when it doesn't overflow.
fn mul(a: I, b: I) -> I { let p = mul32w(a.x, b.x); return I(p.x, p.y + a.x * b.y + a.y * b.x); }
fn muli(a: I, b: i32) -> I { return mul(a, i_(b)); }

// ── 128 bits ──────────────────────────────────────────────────────────────
fn widen(a: I) -> W { let s = select(0u, 0xffffffffu, isneg(a)); return W(a.x, a.y, s, s); }
fn narrow(w: W) -> I { return w.xy; }
fn wneg(a: W) -> W {
    var r = ~a; r.x += 1u;
    if (r.x == 0u) { r.y += 1u; if (r.y == 0u) { r.z += 1u; if (r.z == 0u) { r.w += 1u; } } }
    return r;
}
fn wisneg(a: W) -> bool { return (a.w & SIGN) != 0u; }
fn wabs(a: W) -> W { return select(a, wneg(a), wisneg(a)); }
fn wadd(a: W, b: W) -> W {
    var r: W; var c = 0u;
    r.x = a.x + b.x; c = select(0u, 1u, r.x < a.x);
    let y = a.y + b.y; r.y = y + c; c = select(0u, 1u, y < a.y || r.y < y);
    let z = a.z + b.z; r.z = z + c; c = select(0u, 1u, z < a.z || r.z < z);
    r.w = a.w + b.w + c;
    return r;
}
fn wsub(a: W, b: W) -> W { return wadd(a, wneg(b)); }
// Unsigned 64 × 64 → 128.
fn umulw(a: I, b: I) -> W {
    let p0 = mul32w(a.x, b.x); let p1 = mul32w(a.x, b.y); let p2 = mul32w(a.y, b.x); let p3 = mul32w(a.y, b.y);
    let s1 = add(add(I(p0.y, 0u), I(p1.x, 0u)), I(p2.x, 0u));
    let s2 = add(add(add(I(p1.y, 0u), I(p2.y, 0u)), I(p3.x, 0u)), I(s1.y, 0u));
    return W(p0.x, s1.x, s2.x, p3.y + s2.y);
}
// Signed i64 × i64 → i128.
fn mulw(a: I, b: I) -> W {
    let ua = abs_(a); let ub = abs_(b);
    var r: W;
    if (ua.y == 0u && ub.y == 0u) { let p = mul32w(ua.x, ub.x); r = W(p.x, p.y, 0u, 0u); } else { r = umulw(ua, ub); }
    return select(r, wneg(r), isneg(a) != isneg(b));
}
fn clz64(a: I) -> u32 { if (a.y != 0u) { return countLeadingZeros(a.y); } return 32u + countLeadingZeros(a.x); }
// Unsigned 64 / 64, truncating (d ≠ 0).
fn udiv64(n: I, d: I) -> I {
    if (n.y == 0u && d.y == 0u) { return I(n.x / d.x, 0u); }
    if (d.y == 0u && d.x < 65536u) { return udivw_small(W(n.x, n.y, 0u, 0u), d.x).xy; }
    if (ult(n, d)) { return I(0u, 0u); }
    // Shift-subtract from the numerator's top bit.
    let top = 63u - clz64(n);
    let span = top - (63u - clz64(d));
    var r = n;
    var q = I(0u, 0u);
    var dd = shl64(d, span);
    for (var i = 0u; i <= span; i++) {
        let s = span - i;
        if (!ult(r, dd)) { r = sub(r, dd); if (s >= 32u) { q.y |= 1u << (s - 32u); } else { q.x |= 1u << s; } }
        dd = I((dd.x >> 1u) | (dd.y << 31u), dd.y >> 1u);
    }
    return q;
}
fn shl64(a: I, s: u32) -> I {
    if (s == 0u) { return a; }
    if (s >= 32u) { return I(0u, a.x << (s - 32u)); }
    return I(a.x << s, (a.y << s) | (a.x >> (32u - s)));
}
fn wult(a: W, b: W) -> bool {
    if (a.w != b.w) { return a.w < b.w; }
    if (a.z != b.z) { return a.z < b.z; }
    if (a.y != b.y) { return a.y < b.y; }
    return a.x < b.x;
}
fn wshr1(a: W) -> W { return W((a.x >> 1u) | (a.y << 31u), (a.y >> 1u) | (a.z << 31u), (a.z >> 1u) | (a.w << 31u), a.w >> 1u); }
fn wclz(a: W) -> u32 {
    if (a.w != 0u) { return countLeadingZeros(a.w); }
    if (a.z != 0u) { return 32u + countLeadingZeros(a.z); }
    if (a.y != 0u) { return 64u + countLeadingZeros(a.y); }
    return 96u + countLeadingZeros(a.x);
}
fn wshl(a: W, s: u32) -> W {
    var r = a;
    var k = s;
    while (k >= 32u) { r = W(0u, r.x, r.y, r.z); k -= 32u; }
    if (k == 0u) { return r; }
    return W(r.x << k, (r.y << k) | (r.x >> (32u - k)), (r.z << k) | (r.y >> (32u - k)), (r.w << k) | (r.z >> (32u - k)));
}
// Unsigned 128 / d for 0 < d < 2¹⁶: long division in 16-bit digits, each
// a native 32-bit divide (exact).
fn udivw_small(n: W, d: u32) -> W {
    if (n.y == 0u && n.z == 0u && n.w == 0u) { return W(n.x / d, 0u, 0u, 0u); }
    var q = W(0u, 0u, 0u, 0u);
    var r = 0u;
    let top = (127u - wclz(n)) / 16u;
    for (var j = i32(top); j >= 0; j--) {
        let w = u32(j) / 2u; let sh = 16u * (u32(j) % 2u);
        let cur = (r << 16u) | ((n[w] >> sh) & 0xffffu);
        let ql = cur / d;
        r = cur - ql * d;
        q[w] |= ql << sh;
    }
    return q;
}
// Unsigned 128 / 128, truncating (d ≠ 0).
fn udivw(n: W, d: W) -> W {
    if (d.y == 0u && d.z == 0u && d.w == 0u && d.x < 65536u) { return udivw_small(n, d.x); }
    if (n.z == 0u && n.w == 0u && d.z == 0u && d.w == 0u) { let q = udiv64(n.xy, d.xy); return W(q.x, q.y, 0u, 0u); }
    if (wult(n, d)) { return W(0u, 0u, 0u, 0u); }
    let span = wclz(d) - wclz(n);
    var r = n;
    var q = W(0u, 0u, 0u, 0u);
    var dd = wshl(d, span);
    for (var i = 0u; i <= span; i++) {
        let s = span - i;
        if (!wult(r, dd)) { r = wsub(r, dd); q[s / 32u] |= 1u << (s % 32u); }
        dd = wshr1(dd);
    }
    return q;
}
// i128 / i128, truncating toward zero (Rust's `/`).
fn wdiv(n: W, d: W) -> W {
    let q = udivw(wabs(n), wabs(d));
    return select(q, wneg(q), wisneg(n) != wisneg(d));
}
// i64 / i64, truncating toward zero.
fn div(n: I, d: I) -> I {
    let q = udiv64(abs_(n), abs_(d));
    return select(q, neg_(q), isneg(n) != isneg(d));
}
fn divi(n: I, d: i32) -> I { return div(n, i_(d)); }
// a × b / d through 128 bits (units::mul_div), narrowed to 64.
fn mul_div(a: I, b: I, d: I) -> I { return narrow(wdiv(mulw(a, b), widen(d))); }
// a × b / (d1 × d2), d1, d2 > 0: ⌊⌊x/d1⌋/d2⌋ = ⌊x/(d1·d2)⌋ (and the same
// for truncation toward zero, by symmetry), so a divisor too big for the
// 16-bit path can go as two that fit.
fn mul_div2(a: I, b: I, d1: I, d2: I) -> I { return narrow(wdiv(wdiv(mulw(a, b), widen(d1)), widen(d2))); }
fn mul_div_1e6(a: I, b: I) -> I { return mul_div2(a, b, i_(1000), i_(1000)); }

// Exact floor(√v) for v ≥ 0: what (v as f64).sqrt() as i64 gives for every
// v below 2⁵², which covers buoyant_churn's range by orders of magnitude.
fn isqrt(v: I) -> I {
    if (v.y == 0u) {
        var x = v.x;
        var r = 0u;
        var bit = 1u << 30u;
        while (bit > x) { bit >>= 2u; }
        while (bit != 0u) {
            if (x >= r + bit) { x -= r + bit; r = (r >> 1u) + bit; } else { r >>= 1u; }
            bit >>= 2u;
        }
        return I(r, 0u);
    }
    // 64-bit: bit by bit on the root (32 steps).
    var r = 0u;
    for (var b = 31; b >= 0; b--) {
        let c = r | (1u << u32(b));
        let sq = mul32w(c, c);
        if (!ult(v, sq)) { r = c; }
    }
    return I(r, 0u);
}

// ── what both layouts share: a side, a face's flux, and diffuse_face ──────
// Each layout defines gas(cell, k) -> I, side(cell) -> Side and ambient() -> I.

struct Side {
    t: I, t_cond: I, t_skin: I, gas_ml: I, half: I, excess: I, flame_t: I,
    flame_heat: I, flame_fill: I, fuel_g: I, reached: I, water_g: I,
    permeable: bool, wet: bool,
}
fn gas_mass(c: u32) -> I { var m = I(0u, 0u); for (var k = 0u; k < 7u; k++) { m = add(m, gas(c, k)); } return m; }

struct Flux { energy: I, carried: I, gas: array<I, 7>, flame: I, film: bool }

// Energy carried by gas k of mass m at temperature t.
fn carried(k: u32, m: I, t: I) -> I {
    var e = mul_div_1e6(muli(m, C_GAS_SH[k]), t);
    if (k == C_VAPOUR) { e = add(e, muli(m, C_LATENT)); }
    return e;
}
// conductance × ΔT × tick, µJ.
fn heat(g: I, dt: I) -> I { return mul_div(mul_div(g, dt, i_(1000)), i_(C_TICK), i_(1000)); }

fn buoyant_churn(t: I, ambient: I) -> I {
    let dt = mx(sub(t, ambient), I(0u, 0u));
    if (iszero(dt)) { return I(0u, 0u); }
    let v2 = mul_div(i_(9807 * C_CELL_MM), dt, mx(ambient, i_(1)));
    return isqrt(v2);
}

fn touch(from_: Side, to: Side, axis: u32) -> I {
    if (iszero(to.fuel_g)) { return I(0u, 0u); }
    var share = I(0u, 0u);
    if (!to.permeable) { share = from_.flame_fill; }
    else if (iszero(from_.flame_t)) { share = I(0u, 0u); }
    else if (axis != 2u) { share = divi(muli(from_.reached, C_FLAME_LEAN), 1000); }
    if (iszero(share) || le(i_(C_FLAME_CONTACT), to.t_skin)) { return I(0u, 0u); }
    let g = divi(mul(mn(to.fuel_g, i_(C_FLAME_FACE_G)), share), 1000);
    return mn(heat(g, sub(i_(C_FLAME_CONTACT), to.t_skin)), from_.flame_heat);
}

// transport::diffuse_face: lower cell lo with side a, upper cell hi with side b.
fn diffuse_face(lo: u32, a: Side, hi: u32, b: Side, speed: I, axis: u32) -> Flux {
    var f: Flux;
    f.energy = I(0u, 0u); f.carried = I(0u, 0u); f.flame = I(0u, 0u); f.film = false;
    for (var k = 0u; k < 7u; k++) { f.gas[k] = I(0u, 0u); }
    f.flame = sub(touch(a, b, axis), touch(b, a, axis));

    let ga = widen(mx(a.half, i_(1)));
    let gb = widen(mx(b.half, i_(1)));
    var g = wdiv(mulw(ga.xy, gb.xy), wadd(ga, gb));
    var ta = a.t_cond; var tb = b.t_cond;
    if (a.permeable != b.permeable) {
        let solid = select(ga, gb, a.permeable);
        var film = widen(i_(C_FILM));
        if (axis == 2u && b.wet && !a.permeable) { film = widen(b.water_g); }
        g = wdiv(mulw(solid.xy, film.xy), wadd(solid, film));
        f.film = true;
        if (!a.permeable) { ta = a.t_skin; }
        if (!b.permeable) { tb = b.t_skin; }
    }
    f.energy = heat(narrow(g), sub(ta, tb));

    let both_gas = gt0(a.gas_ml) && gt0(b.gas_ml);
    if (both_gas) {
        let churn = buoyant_churn(mx(a.t, b.t), ambient());
        let q = i_(C_MIX_QUANTUM);
        var ul = mul(div(add(i_(C_STILL_MIX), divi(muli(add(speed, churn), C_TURB_MIX), 1000)), q), q);
        if (!(a.permeable && b.permeable)) { ul = mul(div(divi(ul, C_SEEP), q), q); }
        ul = mn(ul, mul(div(divi(muli(mn(a.gas_ml, b.gas_ml), 1000), 8), q), q));
        for (var k = 0u; k < 7u; k++) {
            let out_a = mul_div2(gas(lo, k), ul, i_(1000), a.gas_ml);
            let out_b = mul_div2(gas(hi, k), ul, i_(1000), b.gas_ml);
            f.gas[k] = add(f.gas[k], sub(out_a, out_b));
            f.carried = add(f.carried, sub(carried(k, out_a, a.t), carried(k, out_b, b.t)));
        }
    }

    if (both_gas && (gt0(a.excess) || gt0(b.excess))) {
        let both_solid = !a.permeable && !b.permeable;
        let vol = mn(a.gas_ml, b.gas_ml);
        let over = wsub(wdiv(mulw(a.excess, vol), widen(a.gas_ml)), wdiv(mulw(b.excess, vol), widen(b.gas_ml)));
        let push = narrow(wdiv(over, widen(i_(select(8, 8 * C_SEEP, both_solid)))));
        let up = !isneg(push);
        let from_ = select(hi, lo, up);
        let t_from = select(b.t, a.t, up);
        let total = gas_mass(from_);
        if (gt0(total) && !iszero(push)) {
            let p = mn(abs_(push), divi(total, 8));
            for (var k = 0u; k < 7u; k++) {
                let m = mul_div(p, gas(from_, k), total);
                let c = carried(k, m, t_from);
                if (up) { f.gas[k] = add(f.gas[k], m); f.carried = add(f.carried, c); }
                else { f.gas[k] = sub(f.gas[k], m); f.carried = sub(f.carried, c); }
            }
        }
    }
    return f;
}

