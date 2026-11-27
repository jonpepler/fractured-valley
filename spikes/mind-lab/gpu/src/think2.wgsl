// Think v2 (`../SPEC-think.md`) as a compute shader. One workgroup of 64 threads per think; each
// workgroup owns one scratch slot and loops over thinks (t = workgroup, + number of workgroups,
// ...), so one dispatch runs every think of a batch, all hops included. Integer only.
//
// Parallel parts: the knots (80 classes), persisting the state, expanding the frontier (one
// thread per sender; flows into a target are summed with atomics, saturating), taking in what a
// hop touched, and every selection (frontier, cap, workspace, output order) by rank counting:
// an element's place is the number of elements that beat it, which is a total order (amount, then
// stable key, then node id), so the result does not depend on thread timing.
// Sequential (thread 0): the injection list, the focus and courage marks.

struct U { count: u32, nwg: u32, nmax: u32, _pad: u32 }

@group(0) @binding(0) var<storage, read> bases: array<u32>;
@group(0) @binding(1) var<storage, read> minds: array<u32>;
@group(0) @binding(2) var<storage, read> inp: array<u32>;
@group(0) @binding(3) var<storage, read> idx: array<u32>;
@group(0) @binding(4) var<storage, read_write> outb: array<u32>;
@group(0) @binding(5) var<storage, read_write> scr: array<atomic<u32>>;
@group(0) @binding(6) var<storage, read> u: U;

const A_CAP: i32 = 16383;
const INC_CAP: u32 = 1073741824u;
const F_CAP: i32 = 262143;
const NONE: u32 = 0xffffffffu;
const FMAX: u32 = 64u;
const SAT_INJECT: u32 = 1u;
const SAT_FRONT: u32 = 2u;
const SAT_INC: u32 = 4u;
const SAT_FACTOR: u32 = 8u;
const MFOCUS: u32 = 1u;
const MCOURAGE: u32 = 2u;

var<workgroup> hdr: array<u32, 28>;
var<workgroup> kn: array<i32, 240>;
var<workgroup> bt: array<i32, 10>;
var<workgroup> front: array<u32, 128>;   // (node, amount) × FMAX
var<workgroup> nfront: u32;
var<workgroup> ncand_s: u32;
var<workgroup> nact: atomic<u32>;
var<workgroup> ntouch: atomic<u32>;
var<workgroup> nkeep: atomic<u32>;
var<workgroup> visits: atomic<u32>;
var<workgroup> flags: atomic<u32>;
var<workgroup> total: atomic<u32>;
var<workgroup> uni: u32;                 // a value every thread must see alike
var<workgroup> fg: i32;
var<workgroup> cg: i32;
// Pointers into `bases` and `minds` for this think: base off, edges, den, tmask, keys, base n;
// mind n, nbase, patch_of, pdesc, pedges, val, keys.
var<workgroup> pt: array<u32, 13>;

// Scratch slot layout (words, n = nmax): act, inc, mark, active list, touched list, candidates
// (node, amount, key), kept list.
var<private> S: u32;
var<private> N: u32;
fn ACT(i: u32) -> u32 { return S + i; }
fn INC(i: u32) -> u32 { return S + N + i; }
fn MARK(i: u32) -> u32 { return S + 2u * N + i; }
fn ACTL(i: u32) -> u32 { return S + 3u * N + i; }
fn TOUCH(i: u32) -> u32 { return S + 4u * N + i; }
fn CAND(i: u32) -> u32 { return S + 5u * N + 3u * i; }
fn KEEP(i: u32) -> u32 { return S + 8u * N + i; }

fn ld(i: u32) -> u32 { return atomicLoad(&scr[i]); }
fn st(i: u32, v: u32) { atomicStore(&scr[i], v); }

const TYPE_GAIN = array<i32, 10>(1000, 1000, 1000, 1000, 700, 600, 1000, 900, 900, 900);

fn key_of(n: u32) -> u32 {
    if (n < pt[12u]) { return bases[pt[4] + n]; }
    return minds[pt[11] + n - pt[12u]];
}
fn val_of(n: u32) -> i32 {
    let w = minds[pt[10] + n / 2u];
    let h = (w >> (16u * (n & 1u))) & 0xffffu;
    return (bitcast<i32>(h << 16u)) >> 16u;
}
// Node n's links: (first edge word, count, sum, type mask, from the mind's patches?).
struct NodeL { e: u32, len: u32, den: u32, tmask: u32, own: bool }
fn node_links(n: u32) -> NodeL {
    let p = minds[pt[7] + n];
    if (p == NONE) {
        if (n < pt[5]) {
            let o0 = bases[pt[0] + 2u + n]; let o1 = bases[pt[0] + 2u + n + 1u];
            return NodeL(pt[1] + 2u * o0, o1 - o0, bases[pt[2] + n], bases[pt[3] + n], false);
        }
        return NodeL(0u, 0u, 1000u, 0u, false);
    }
    let d = pt[8] + 4u * p;
    return NodeL(pt[9] + 2u * minds[d], minds[d + 1u], minds[d + 2u], minds[d + 3u], true);
}
fn edge_word(l: NodeL, i: u32, w: u32) -> u32 {
    if (l.own) { return minds[l.e + 2u * i + w]; }
    return bases[l.e + 2u * i + w];
}

// a beats b: larger amount, then smaller key, then smaller node.
fn beats(an: u32, aa: u32, ak: u32, bn: u32, ba: u32, bk: u32) -> bool {
    return aa > ba || (aa == ba && (ak < bk || (ak == bk && an < bn)));
}

fn factor(c: u32, v: i32) -> i32 {
    let f0 = kn[c * 3u]; let f1 = kn[c * 3u + 1u]; let f2 = kn[c * 3u + 2u];
    if (v < 0) { return f1 + (f0 - f1) * (-v) / 1000; }
    return f1 + (f2 - f1) * v / 1000;
}

// Candidates 0..nc (node, amount, key) → the frontier: the best `k`, by rank.
fn select_frontier(lid: u32, nc: u32, k: u32) {
    for (var i = lid; i < nc; i += 64u) {
        let c = CAND(i);
        let n = ld(c); let a = ld(c + 1u); let kk = ld(c + 2u);
        var r = 0u;
        for (var j = 0u; j < nc; j += 1u) {
            let d = CAND(j);
            if (beats(ld(d), ld(d + 1u), ld(d + 2u), n, a, kk)) { r += 1u; if (r >= k) { break; } }
        }
        if (r < k) { front[2u * r] = n; front[2u * r + 1u] = a; }
    }
}

fn think_one(t: u32, lid: u32) {
    let h = idx[t];
    if (lid < 28u) { hdr[lid] = inp[h + lid]; }
    if (lid == 0u) { atomicStore(&nact, 0u); atomicStore(&ntouch, 0u); atomicStore(&nkeep, 0u); atomicStore(&visits, 0u); atomicStore(&flags, 0u); atomicStore(&total, 0u); }
    workgroupBarrier();
    if (lid == 0u) {
        let b = hdr[1]; let bn = bases[b]; let bne = bases[b + 1u];
        pt[0] = b; pt[1] = b + 2u + bn + 1u; pt[2] = pt[1] + 2u * bne; pt[3] = pt[2] + bn; pt[4] = pt[3] + bn; pt[5] = bn;
        let m = hdr[0]; let mn = minds[m]; let np = minds[m + 2u]; let npe = minds[m + 3u];
        pt[6] = mn; pt[12] = minds[m + 1u];
        pt[7] = m + 4u; pt[8] = pt[7] + mn; pt[9] = pt[8] + 4u * np; pt[10] = pt[9] + 2u * npe; pt[11] = pt[10] + (mn + 1u) / 2u;
    }
    // Knots (§3).
    let hd = clamp(bitcast<i32>(hdr[15]), 0, 1000);
    let a = clamp(bitcast<i32>(hdr[5]), 0, 1000);
    let hg = clamp(max(bitcast<i32>(hdr[3]), bitcast<i32>(hdr[4])), 0, 1000);
    let fa = clamp(bitcast<i32>(hdr[7]), 0, 1000);
    let s = clamp(bitcast<i32>(hdr[8]), 0, 1000);
    let mood = clamp(bitcast<i32>(hdr[6]), -1000, 1000);
    let neg = max(-mood, 0); let pos = max(mood, 0);
    let fk = 1000 - fa / 3;
    for (var c = lid; c < 80u; c += 64u) {
        let ty = c / 8u; let tc = c % 8u;
        var x = TYPE_GAIN[ty];
        if (ty == 4u || ty == 5u || ty == 7u || ty == 8u || ty == 9u) { x = x * (1000 - a / 2) / 1000 * (1000 - s / 2) / 1000; }
        if ((tc & 4u) != 0u) { x += x * s * 2 / 1000 + x * fa / 1000; }
        if ((tc & 1u) != 0u) { x += x * hg / 1000; }
        if ((tc & 2u) != 0u) { x += x * fa / 1000; }
        let g0 = max(x * fk / 1000, 0);
        kn[c * 3u] = min(g0 * (1000 + 2 * (a + neg)) / 1000 * hd / 1000, F_CAP);
        kn[c * 3u + 1u] = min(g0 * 1000 / 1000 * hd / 1000, F_CAP);
        kn[c * 3u + 2u] = min(g0 * (1000 + pos - neg) / 1000 * hd / 1000, F_CAP);
    }
    workgroupBarrier();
    if (lid < 10u) {
        var b = 0;
        for (var j = lid * 24u; j < lid * 24u + 24u; j += 1u) { b = max(b, kn[j]); }
        bt[lid] = b;
    }
    // Focus and courage marks (thread 0): the intent and its targets; the ally's targets.
    if (lid == 0u) {
        fg = 1000; cg = 1000;
        let focus = bitcast<i32>(hdr[9]); let intent = hdr[10]; let courage = bitcast<i32>(hdr[11]); let ally = hdr[12];
        if (focus > 0 && intent < pt[6]) {
            fg = 1000 + clamp(focus, 0, 1000);
            st(MARK(intent), ld(MARK(intent)) | MFOCUS);
            let l = node_links(intent);
            for (var i = 0u; i < l.len; i += 1u) { let to = edge_word(l, i, 0u); st(MARK(to), ld(MARK(to)) | MFOCUS); }
        }
        if (courage > 0 && ally < pt[6]) {
            let l = node_links(ally);
            if (l.len > 0u) { cg = 1000 + clamp(courage, 0, 1000); }
            for (var i = 0u; i < l.len; i += 1u) { let to = edge_word(l, i, 0u); st(MARK(to), ld(MARK(to)) | MCOURAGE); }
        }
    }
    storageBarrier(); workgroupBarrier();
    let persist = bitcast<i32>(hdr[16]); let carry = bitcast<i32>(hdr[17]);
    let mina = bitcast<i32>(hdr[18]); let maxa = bitcast<i32>(hdr[19]);
    // 1. Persist; 2. seeds from the carried share (each state node appears once).
    let so = hdr[23]; let sl = hdr[24];
    for (var k = lid; k < sl; k += 64u) {
        let n = inp[so + 2u * k]; let x0 = bitcast<i32>(inp[so + 2u * k + 1u]);
        if (x0 > maxa) { atomicOr(&flags, SAT_INJECT); }
        let x = clamp(x0, 0, maxa) * persist / 1000;
        if (x >= mina) {
            st(ACT(n), u32(x));
            st(ACTL(atomicAdd(&nact, 1u)), n);
            let c = x * carry / 1000;
            if (c > 0) { st(INC(n), u32(c)); st(TOUCH(atomicAdd(&ntouch, 1u)), n); }
        }
    }
    storageBarrier(); workgroupBarrier();
    // The injections, in order (thread 0).
    if (lid == 0u) {
        let io = hdr[25]; let ni = hdr[26];
        for (var k = 0u; k < ni; k += 1u) {
            let n = inp[io + 2u * k]; let a0 = bitcast<i32>(inp[io + 2u * k + 1u]);
            if (a0 > A_CAP) { atomicOr(&flags, SAT_INJECT); }
            let av = clamp(a0, 0, A_CAP);
            let x = bitcast<i32>(ld(ACT(n)));
            if (x == 0 && av > 0) { st(ACTL(atomicAdd(&nact, 1u)), n); }
            st(ACT(n), u32(min(x + av, maxa)));
            if (av > 0) {
                let i0 = ld(INC(n));
                if (i0 == 0u) { st(TOUCH(atomicAdd(&ntouch, 1u)), n); }
                st(INC(n), min(i0 + u32(av), INC_CAP));
            }
        }
    }
    storageBarrier(); workgroupBarrier();
    // Seeds → candidates.
    var nt = atomicLoad(&ntouch);
    for (var k = lid; k < nt; k += 64u) {
        let n = ld(TOUCH(k));
        let v = ld(INC(n)); st(INC(n), 0u);
        if (v > u32(A_CAP)) { atomicOr(&flags, SAT_FRONT); }
        let c = CAND(k); st(c, n); st(c + 1u, min(v, u32(A_CAP))); st(c + 2u, key_of(n));
    }
    storageBarrier(); workgroupBarrier();
    let fcap = min(hdr[14], FMAX);
    if (lid == 0u) { ncand_s = nt; nfront = min(nt, fcap); atomicStore(&ntouch, 0u); }
    workgroupBarrier();
    select_frontier(lid, nt, fcap);
    workgroupBarrier();
    // 3. Hops.
    let lim = 1000u * u32(mina);
    let hops = hdr[13];
    var hops_run = 0u;
    for (var hop = 0u; hop < hops; hop += 1u) {
        let nf = workgroupUniformLoad(&nfront);
        if (nf == 0u) { break; }
        hops_run += 1u;
        for (var i = lid; i < nf; i += 64u) {
            let su = front[2u * i]; let sa = front[2u * i + 1u];
            let l = node_links(su);
            atomicAdd(&visits, l.len);
            var b = 0; var mm = l.tmask;
            while (mm != 0u) { let ty = firstTrailingBit(mm); mm = mm & (mm - 1u); b = max(b, bt[ty]); }
            let bound = u32(min(b * fg / 1000 * cg / 1000, F_CAP));
            for (var e = 0u; e < l.len; e += 1u) {
                let to = edge_word(l, e, 0u); let pk = edge_word(l, e, 1u);
                let w = pk & 1023u;
                let q = sa * w / l.den;
                if (q * bound < lim) { break; }
                let lv = i32((pk >> 17u) & 2047u) - 1024;
                let v = clamp(val_of(to) + lv, -1000, 1000);
                var f = factor((pk >> 10u) & 127u, v);
                let mk = ld(MARK(to));
                if ((mk & MFOCUS) != 0u && fg != 1000) { f = f * fg / 1000; }
                if ((mk & MCOURAGE) != 0u && cg != 1000) { f = f * cg / 1000; }
                if (f > F_CAP) { atomicOr(&flags, SAT_FACTOR); f = F_CAP; }
                let fl = q * u32(f) / 1000u;
                if (fl < u32(mina)) { continue; }
                let old = atomicAdd(&scr[INC(to)], fl);
                if (old + fl > INC_CAP) { atomicMin(&scr[INC(to)], INC_CAP); atomicOr(&flags, SAT_INC); }
                if (old == 0u) { st(TOUCH(atomicAdd(&ntouch, 1u)), to); }
            }
        }
        storageBarrier(); workgroupBarrier();
        nt = atomicLoad(&ntouch);
        for (var k = lid; k < nt; k += 64u) {
            let n = ld(TOUCH(k));
            let raw = ld(INC(n)); st(INC(n), 0u);
            let x = ld(ACT(n));
            if (x == 0u) { st(ACTL(atomicAdd(&nact, 1u)), n); }
            st(ACT(n), min(x + raw, u32(maxa)));
            if (raw > u32(A_CAP)) { atomicOr(&flags, SAT_FRONT); }
            let c = CAND(k); st(c, n); st(c + 1u, min(raw, u32(A_CAP))); st(c + 2u, key_of(n));
        }
        storageBarrier(); workgroupBarrier();
        if (lid == 0u) { nfront = min(nt, fcap); atomicStore(&ntouch, 0u); }
        workgroupBarrier();
        select_frontier(lid, nt, fcap);
        workgroupBarrier();
    }
    // 4. Budget, cap, workspace, output.
    let na = atomicLoad(&nact);
    for (var k = lid; k < na; k += 64u) { atomicAdd(&total, ld(ACT(ld(ACTL(k))))); }
    workgroupBarrier();
    let tot = atomicLoad(&total);
    let budget = hdr[22];
    for (var k = lid; k < na; k += 64u) {
        let n = ld(ACTL(k));
        var x = ld(ACT(n));
        if (tot > budget) {
            x = x * budget / tot;
            if (x >= u32(mina)) { st(ACT(n), x); st(KEEP(atomicAdd(&nkeep, 1u)), n); } else { st(ACT(n), 0u); }
        } else { st(KEEP(atomicAdd(&nkeep, 1u)), n); }
    }
    storageBarrier(); workgroupBarrier();
    let nk = atomicLoad(&nkeep);
    let cap = hdr[21]; let wsk = hdr[20];
    // Each kept node's rank by (activation, key, node): dropped past the cap; then its rank among
    // the survivors by node id (the output order) and, if within ws_k, its workspace place.
    let oo = hdr[2];
    for (var k = lid; k < nk; k += 64u) {
        let n = ld(KEEP(k)); let x = ld(ACT(n)); let kk = key_of(n);
        var r = 0u;
        for (var j = 0u; j < nk; j += 1u) { let m = ld(KEEP(j)); if (beats(m, ld(ACT(m)), key_of(m), n, x, kk)) { r += 1u; } }
        if (r < cap) {
            if (r < wsk) { outb[oo + 6u + r] = n; }
            st(CAND(k), r);
        } else { st(CAND(k), NONE); }
    }
    storageBarrier(); workgroupBarrier();
    let ns = min(nk, cap);
    for (var k = lid; k < nk; k += 64u) {
        let n = ld(KEEP(k));
        if (ld(CAND(k)) == NONE) { continue; }
        var p = 0u;
        for (var j = 0u; j < nk; j += 1u) { let m = ld(KEEP(j)); if (m < n && ld(CAND(j)) != NONE) { p += 1u; } }
        outb[oo + 6u + wsk + 2u * p] = n; outb[oo + 6u + wsk + 2u * p + 1u] = ld(ACT(n));
    }
    storageBarrier(); workgroupBarrier();
    if (lid == 0u) {
        outb[oo] = atomicLoad(&flags); outb[oo + 1u] = atomicLoad(&visits); outb[oo + 2u] = hops_run;
        outb[oo + 3u] = min(ns, wsk); outb[oo + 4u] = ns; outb[oo + 5u] = wsk;
        // Clear the marks.
        let intent = hdr[10]; let ally = hdr[12];
        if (bitcast<i32>(hdr[9]) > 0 && intent < pt[6]) {
            st(MARK(intent), 0u);
            let l = node_links(intent);
            for (var i = 0u; i < l.len; i += 1u) { st(MARK(edge_word(l, i, 0u)), 0u); }
        }
        if (bitcast<i32>(hdr[11]) > 0 && ally < pt[6]) {
            let l = node_links(ally);
            for (var i = 0u; i < l.len; i += 1u) { st(MARK(edge_word(l, i, 0u)), 0u); }
        }
    }
    // Leave the slot clean: every activation back to 0.
    for (var k = lid; k < nk; k += 64u) { st(ACT(ld(KEEP(k))), 0u); }
    storageBarrier(); workgroupBarrier();
}

@compute @workgroup_size(64)
fn main(@builtin(workgroup_id) wid: vec3<u32>, @builtin(local_invocation_index) lid: u32) {
    N = u.nmax;   // the scratch stride (nodes per slot), fixed by the host
    S = wid.x * 9u * N;
    for (var t = wid.x; t < u.count; t += u.nwg) {
        think_one(t, lid);
    }
}
