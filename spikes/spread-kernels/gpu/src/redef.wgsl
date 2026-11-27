// Track B (redefined) step on the GPU: one invocation per instance, integer arithmetic only.
// Mirrors `redef::step_with_knots` in the CPU crate (same results, tested), except that edge
// pruning uses a per-edge test (q * fmax < 1000 min -> stop) instead of a 64-bit weight bound.

struct P {
    count: u32, first: u32, hops: u32, frontier: u32,
    persist: i32, carry: i32, min_act: i32, max_act: i32,
    budget: i32, active_cap: u32, ws_k: u32, hop_decay: i32,
    gen: u32, tick: u32, ticks: u32, _pad: u32,
}

@group(0) @binding(0) var<storage, read> sh: array<u32>;
@group(0) @binding(1) var<storage, read> ib: array<u32>;
@group(0) @binding(2) var<storage, read_write> st: array<u32>;
@group(0) @binding(3) var<storage, read> inp: array<u32>;
@group(0) @binding(4) var<storage, read_write> outb: array<u32>;
@group(0) @binding(5) var<storage, read_write> scr: array<u32>;
@group(0) @binding(6) var<storage, read> p: P; // storage, not uniform: one buffer kind throughout

const TS: u32 = 1024u;      // table slots
const LCAP: u32 = 512u;     // list capacity (and table load limit)
const SCR: u32 = 9216u;
const ST: u32 = 513u;
const IN: u32 = 146u;
const OUT: u32 = 10u;
const A_CAP: i32 = 65535;
const INC_CAP: i32 = 1073741824;

var<private> base: u32;     // scratch base of this invocation
var<private> nused: u32;
var<private> ntouched: u32;
var<private> overflow: u32;
var<private> kn: array<i32, 96>;
var<private> fmx: array<i32, 32>;

fn T(i: u32) -> u32 { return base + i * 6u; }           // table slot fields: node gen act inc seed flags
fn USED() -> u32 { return base + TS * 6u; }
fn TOUCH() -> u32 { return USED() + LCAP; }
fn CAND() -> u32 { return TOUCH() + LCAP; }
fn CC() -> u32 { return CAND() + 2u * LCAP; }

fn slot(n: u32) -> u32 {
    var i = (n * 0x9e3779b1u) >> 10u;
    loop {
        i = i & (TS - 1u);
        let s = T(i);
        if (scr[s + 1u] != p.gen) {
            if (nused >= LCAP) { overflow = 1u; return 0xffffffffu; }
            scr[s] = n; scr[s + 1u] = p.gen; scr[s + 2u] = 0u; scr[s + 3u] = 0u; scr[s + 4u] = 0u; scr[s + 5u] = 0u;
            scr[USED() + nused] = i; nused += 1u;
            return s;
        }
        if (scr[s] == n) { return s; }
        i += 1u;
    }
    return 0xffffffffu;
}

// larger amount first, then smaller id
fn better(a1: i32, n1: u32, a2: i32, n2: u32) -> bool { return a1 > a2 || (a1 == a2 && n1 < n2); }

// Keep the k best of cand[0..n] (unordered); returns the new length.
fn select_top(off: u32, n: u32, k: u32) -> u32 {
    if (n <= k) { return n; }
    for (var j = 0u; j < k; j += 1u) {
        var b = j;
        for (var i = j + 1u; i < n; i += 1u) {
            if (better(bitcast<i32>(scr[off + 2u * i + 1u]), scr[off + 2u * i], bitcast<i32>(scr[off + 2u * b + 1u]), scr[off + 2u * b])) { b = i; }
        }
        let tn = scr[off + 2u * j]; let ta = scr[off + 2u * j + 1u];
        scr[off + 2u * j] = scr[off + 2u * b]; scr[off + 2u * j + 1u] = scr[off + 2u * b + 1u];
        scr[off + 2u * b] = tn; scr[off + 2u * b + 1u] = ta;
    }
    return k;
}

fn factor(key: u32) -> i32 {
    let c = key >> 11u;
    let v = i32(key & 2047u) - 1000;
    let f0 = kn[c * 3u]; let f1 = kn[c * 3u + 1u]; let f2 = kn[c * 3u + 2u];
    if (v < 0) { return f1 + (f0 - f1) * (-v) / 1000; }
    return f1 + (f2 - f1) * v / 1000;
}

fn edge(a: u32, den: u32, to: u32, w: i32, key: u32) {
    if (w <= 0) { return; }
    let q = a * u32(w) / den;
    let f = factor(key);
    if (f <= 0) { return; }
    let fl = i32(q * u32(f) / 1000u);
    if (fl < p.min_act) { return; }
    let s = slot(to);
    if (s == 0xffffffffu) { return; }
    let inc = bitcast<i32>(scr[s + 3u]);
    if (inc == 0) {
        if (ntouched >= LCAP) { overflow = 1u; return; }
        scr[TOUCH() + ntouched] = to; ntouched += 1u;
    }
    scr[s + 3u] = bitcast<u32>(min(inc + fl, INC_CAP));
}

fn wbits(x: u32) -> i32 { return (bitcast<i32>(x << 16u)) >> 16u; } // sign-extend low 16 bits

// rank lookup: patched index or 0xffffffff
fn find(rb: u32, u: u32) -> u32 {
    let b = rb + (u >> 6u) * 3u;
    let bit = u & 63u;
    let lo = ib[b]; let hi = ib[b + 1u];
    var word = lo; var sh_ = bit;
    if (bit >= 32u) { word = hi; sh_ = bit - 32u; }
    if (((word >> sh_) & 1u) == 0u) { return 0xffffffffu; }
    var cnt = 0u;
    if (bit >= 32u) { cnt = countOneBits(lo) + countOneBits(hi & ((1u << (bit - 32u)) - 1u)); }
    else { cnt = countOneBits(lo & ((1u << bit) - 1u)); }
    return ib[b + 2u] + cnt;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let li = gid.x;
    if (li >= p.count) { return; }
    let inst = li; // buffers are per chunk
    base = li * SCR;
    nused = 0u; ntouched = 0u; overflow = 0u;
    let h = inst * 8u;
    let base_n = ib[h]; let rb = ib[h + 1u]; let pb = ib[h + 2u]; let ob = ib[h + 3u]; let xb = ib[h + 4u]; let ovb = ib[h + 5u]; let exb = ib[h + 6u];
    let io = inst * IN;
    for (var c = 0u; c < 32u; c += 1u) {
        let a = bitcast<i32>(inp[io + c * 3u]); let b = bitcast<i32>(inp[io + c * 3u + 1u]); let d = bitcast<i32>(inp[io + c * 3u + 2u]);
        kn[c * 3u] = a; kn[c * 3u + 1u] = b; kn[c * 3u + 2u] = d;
        fmx[c] = max(a, max(b, d));
    }
    let damp = bitcast<i32>(inp[io + 96u]);
    let ninj = inp[io + 97u];
    let so = inst * ST;
    let slen = st[so];
    // damp + load + decay + seeds
    for (var k = 0u; k < slen; k += 1u) {
        let n = st[so + 1u + 2u * k];
        var x = bitcast<i32>(st[so + 2u + 2u * k]);
        if (damp != 1000) { x = x * damp / 1000; }
        let s = slot(n);
        x = x * p.persist / 1000;
        if (s != 0xffffffffu && x >= p.min_act) {
            scr[s + 2u] = bitcast<u32>(x);
            var fl = 1u;
            let sd = x * p.carry / 1000;
            if (sd > 0) { scr[s + 4u] = bitcast<u32>(sd); fl = 3u; }
            scr[s + 5u] = fl;
        }
    }
    for (var k = 0u; k < ninj; k += 1u) {
        let n = inp[io + 98u + 2u * k];
        let a = clamp(bitcast<i32>(inp[io + 99u + 2u * k]), 0, A_CAP);
        let s = slot(n);
        if (s == 0xffffffffu) { break; }
        let x = bitcast<i32>(scr[s + 2u]);
        var fl = scr[s + 5u];
        if (x == 0 && a > 0) { fl = fl | 1u; }
        scr[s + 2u] = bitcast<u32>(min(x + a, p.max_act));
        scr[s + 4u] = bitcast<u32>(min(bitcast<i32>(scr[s + 4u]) + a, A_CAP));
        scr[s + 5u] = fl | 2u;
    }
    var nc = 0u;
    for (var k = 0u; k < nused; k += 1u) {
        let s = T(scr[USED() + k]);
        let sd = bitcast<i32>(scr[s + 4u]);
        if ((scr[s + 5u] & 2u) != 0u && sd > 0) { scr[CAND() + 2u * nc] = scr[s]; scr[CAND() + 2u * nc + 1u] = bitcast<u32>(sd); nc += 1u; }
    }
    nc = select_top(CAND(), nc, p.frontier);
    for (var r = 0u; r < p.hops; r += 1u) {
        if (nc == 0u || overflow != 0u) { break; }
        for (var k = 0u; k < nc; k += 1u) {
            let u = scr[CAND() + 2u * k];
            let a = scr[CAND() + 2u * k + 1u];
            let pi = find(rb, u);
            var den = 0u; var mask = 0u;
            if (pi == 0xffffffffu) {
                if (u >= base_n) { continue; }
                den = bitcast<u32>(sh[sh[3] + u]); mask = sh[sh[4] + u];
            } else {
                den = ib[pb + 2u * pi]; mask = ib[pb + 2u * pi + 1u];
            }
            var fmax = 0;
            var mm = mask;
            while (mm != 0u) { let c = firstTrailingBit(mm); mm = mm & (mm - 1u); fmax = max(fmax, fmx[c]); }
            if (fmax <= 0) { continue; }
            let lim = 1000u * u32(p.min_act);
            if (pi == 0xffffffffu) {
                let e0 = sh[sh[1] + u]; let e1 = sh[sh[1] + u + 1u];
                for (var e = e0; e < e1; e += 1u) {
                    let to = sh[sh[2] + 2u * e]; let wk = sh[sh[2] + 2u * e + 1u];
                    let w = wbits(wk);
                    if (w <= 0) { break; }
                    if ((a * u32(w) / den) * u32(fmax) < lim) { break; } // sorted heaviest first
                    edge(a, den, to, w, wk >> 16u);
                }
            } else {
                let os = ib[ob + pi]; let oe = ib[ob + pi + 1u];
                var j = os;
                if (u < base_n) {
                    let e0 = sh[sh[1] + u]; let e1 = sh[sh[1] + u + 1u];
                    for (var e = e0; e < e1; e += 1u) {
                        let to = sh[sh[2] + 2u * e];
                        var wk = sh[sh[2] + 2u * e + 1u];
                        var w = wbits(wk); var key = wk >> 16u;
                        if (j < oe && (ib[ovb + 2u * j] & 0xffffu) == e - e0) { w = wbits(ib[ovb + 2u * j] >> 16u); key = ib[ovb + 2u * j + 1u]; j += 1u; }
                        if (w <= 0 || (a * u32(w) / den) * u32(fmax) < lim) { continue; }
                        edge(a, den, to, w, key);
                    }
                }
                let xs = ib[xb + pi]; let xe = ib[xb + pi + 1u];
                for (var e = xs; e < xe; e += 1u) {
                    let to = ib[exb + 2u * e]; let wk = ib[exb + 2u * e + 1u];
                    let w = wbits(wk);
                    if (w <= 0 || (a * u32(w) / den) * u32(fmax) < lim) { continue; }
                    edge(a, den, to, w, wk >> 16u);
                }
            }
        }
        nc = 0u;
        for (var k = 0u; k < ntouched; k += 1u) {
            let n = scr[TOUCH() + k];
            let s = slot(n);
            let v = bitcast<i32>(scr[s + 3u]);
            scr[s + 3u] = 0u;
            let x = bitcast<i32>(scr[s + 2u]);
            if (x == 0 && v > 0) { scr[s + 5u] = scr[s + 5u] | 1u; }
            scr[s + 2u] = bitcast<u32>(min(x + v, p.max_act));
            scr[CAND() + 2u * nc] = n; scr[CAND() + 2u * nc + 1u] = bitcast<u32>(min(v, A_CAP)); nc += 1u;
        }
        ntouched = 0u;
        nc = select_top(CAND(), nc, p.frontier);
    }
    let oo = inst * OUT;
    if (overflow != 0u) { outb[oo + 9u] = 1u; return; }
    // finish
    var n2 = 0u; var tot = 0;
    for (var k = 0u; k < nused; k += 1u) {
        let s = T(scr[USED() + k]);
        if ((scr[s + 5u] & 1u) != 0u) { let x = bitcast<i32>(scr[s + 2u]); scr[CC() + 2u * n2] = scr[s]; scr[CC() + 2u * n2 + 1u] = bitcast<u32>(x); n2 += 1u; tot += x; }
    }
    if (tot > p.budget) {
        var w = 0u;
        for (var k = 0u; k < n2; k += 1u) {
            let x = bitcast<i32>(scr[CC() + 2u * k + 1u]) * p.budget / tot;
            if (x >= p.min_act) { scr[CC() + 2u * w] = scr[CC() + 2u * k]; scr[CC() + 2u * w + 1u] = bitcast<u32>(x); w += 1u; }
        }
        n2 = w;
    }
    n2 = select_top(CC(), n2, p.active_cap);
    // sort by node (insertion)
    for (var i = 1u; i < n2; i += 1u) {
        let kn_ = scr[CC() + 2u * i]; let kx = scr[CC() + 2u * i + 1u];
        var j = i;
        while (j > 0u && scr[CC() + 2u * (j - 1u)] > kn_) { scr[CC() + 2u * j] = scr[CC() + 2u * (j - 1u)]; scr[CC() + 2u * j + 1u] = scr[CC() + 2u * (j - 1u) + 1u]; j -= 1u; }
        scr[CC() + 2u * j] = kn_; scr[CC() + 2u * j + 1u] = kx;
    }
    st[so] = n2;
    for (var k = 0u; k < n2; k += 1u) { st[so + 1u + 2u * k] = scr[CC() + 2u * k]; st[so + 2u + 2u * k] = scr[CC() + 2u * k + 1u]; }
    // output: top ws_k by (x desc, id asc) among x > 0, in order
    var no = 0u;
    for (var k = 0u; k < n2; k += 1u) {
        let x = bitcast<i32>(scr[CC() + 2u * k + 1u]);
        if (x > 0) { scr[CAND() + 2u * no] = scr[CC() + 2u * k]; scr[CAND() + 2u * no + 1u] = bitcast<u32>(x); no += 1u; }
    }
    let kk = min(no, p.ws_k);
    // selection sort the first kk
    for (var j = 0u; j < kk; j += 1u) {
        var b = j;
        for (var i = j + 1u; i < no; i += 1u) {
            if (better(bitcast<i32>(scr[CAND() + 2u * i + 1u]), scr[CAND() + 2u * i], bitcast<i32>(scr[CAND() + 2u * b + 1u]), scr[CAND() + 2u * b])) { b = i; }
        }
        let tn = scr[CAND() + 2u * j]; let ta = scr[CAND() + 2u * j + 1u];
        scr[CAND() + 2u * j] = scr[CAND() + 2u * b]; scr[CAND() + 2u * j + 1u] = scr[CAND() + 2u * b + 1u];
        scr[CAND() + 2u * b] = tn; scr[CAND() + 2u * b + 1u] = ta;
    }
    outb[oo] = kk;
    for (var j = 0u; j < kk; j += 1u) { outb[oo + 1u + j] = scr[CAND() + 2u * j]; }
    outb[oo + 9u] = 0u;
}
