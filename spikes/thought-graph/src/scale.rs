//! Random but structured minds for measuring cost at 1k–100k nodes.
//!
//! Nodes come in communities of 64 (a neighbourhood of related places, things and ideas).
//! Each node has 6–18 out-links: 80% inside its community, 15% to the next community, 5%
//! anywhere. About 8% of nodes are places with coordinates (community centre plus jitter), 12%
//! are things, the rest connectors. 10% are food, 5% danger, 5% rest; 40% carry a valence.

use crate::graph::{lt, tag, Anchor, Builder, Graph};
use crate::hd::Hd;
use crate::packed::Packed;
use crate::rng::hash;
use crate::spread::{self, Activation, Live, Mods, Params};
use std::time::Instant;

pub const COMMUNITY: u64 = 64;

pub fn structured(n: usize, seed: u64) -> Graph {
    let mut b = Builder::default();
    let h = |i: u64, salt: u64| hash(seed, i, salt, 0x5ca1e);
    let communities = (n as u64).div_ceil(COMMUNITY);
    let mut side = communities.isqrt() as i64;
    if (side * side) < communities as i64 { side += 1; }
    for i in 0..n as u64 {
        let r = h(i, 1) % 100;
        let c = i / COMMUNITY;
        let (cx, cy) = ((c as i64 % side) * 40, (c as i64 / side) * 40);
        let (jx, jy) = ((h(i, 2) % 40) as i64, (h(i, 3) % 40) as i64);
        let (x, y) = ((cx + jx).clamp(-32000, 32000) as i16, (cy + jy).clamp(-32000, 32000) as i16);
        let mut tags = 0u8;
        let anchor = if r < 8 { tags |= tag::PLACE; Some(Anchor::Place { x, y }) }
            else if r < 20 { Some(Anchor::Entity { id: i as u32, x, y }) } else { None };
        let t = h(i, 4) % 100;
        if t < 10 { tags |= tag::FOOD } else if t < 15 { tags |= tag::DANGER } else if t < 20 { tags |= tag::REST }
        let val = if h(i, 5) % 100 < 40 { (h(i, 6) % 1401) as i32 - 700 } else { 0 };
        b.node("", val, tags, anchor);
    }
    for i in 0..n as u64 {
        let deg = 6 + h(i, 7) % 13;
        let c = i / COMMUNITY;
        for k in 0..deg {
            let r = h(i, 100 + k) % 100;
            let pick = h(i, 200 + k);
            let t = if r < 80 { c * COMMUNITY + pick % COMMUNITY }
                else if r < 95 { ((c + 1) % communities) * COMMUNITY + pick % COMMUNITY }
                else { pick % n as u64 };
            let t = t.min(n as u64 - 1);
            if t == i { continue; }
            let (ti, tt) = (b.tags[i as usize], b.tags[t as usize]);
            let ty = crate::hd::infer_type(ti, tt);
            let ty = if ty == lt::KIND && h(i, 300 + k) % 3 == 0 { lt::ASSOC } else { ty };
            b.link(i as u32, t as u32, ty, 50 + (h(i, 400 + k) % 851) as i32);
        }
    }
    b.build()
}

#[derive(Clone, Debug)]
pub struct Row {
    pub nodes: usize,
    pub links: usize,
    /// Packed graph structure, plus fast weights.
    pub graph_bytes: usize,
    /// Compiled bundles plus per-node metadata.
    pub hd_bytes: usize,
    /// The shared codebook (regenerable from the seed).
    pub codebook_bytes: usize,
    /// Activation state, the same for either form.
    pub state_bytes: usize,
    /// The quantised packed form.
    pub packed_bytes: usize,
    pub ns_graph: u128,
    pub ns_hd: u128,
    pub ns_packed: u128,
    /// HD at half the bits (1024 when `bits` is 2048): size and agreement.
    pub hd_half_bytes: usize,
    pub half_same_top1: usize,
    pub half_ws_overlap_pm: usize,
    pub packed_same_top1: usize,
    pub compile_ms: u128,
    /// Ticks compared, same top-1, mean overlap of the workspace (per mille).
    pub compared: usize,
    pub same_top1: usize,
    pub ws_overlap_pm: usize,
}

fn mods_at(seed: u64, t: u64) -> Mods {
    Mods { hunger: (hash(seed, t, 1, 9) % 1001) as i32, arousal: (hash(seed, t, 2, 9) % 1001) as i32,
        mood: (hash(seed, t, 3, 9) % 1601) as i32 - 800, fatigue: (hash(seed, t, 4, 9) % 601) as i32, stress: 0 }
}
fn inject_at(seed: u64, t: u64, n: usize) -> Vec<(u32, i32)> {
    (0..4).map(|k| ((hash(seed, t, k, 11) % n as u64) as u32, 600 + (hash(seed, t, k, 12) % 401) as i32)).collect()
}

/// Measure one size. `hd_thinks` is kept small at 100k: the compiled think scans every node.
pub fn bench(n: usize, bits: usize, graph_thinks: u64, hd_thinks: u64, compare: u64, seed: u64) -> Row {
    let g = structured(n, seed);
    let fast = vec![0i16; g.link_count()];
    let live = Live { g: &g, fast: &fast };
    let p = Params::default();
    let t0 = Instant::now();
    let hd = Hd::compile(&live, &g, bits, seed);
    let compile_ms = t0.elapsed().as_millis();

    let mut st = Activation::new(n);
    for t in 0..20 { spread::think(&live, &mut st, &mods_at(seed, t), &inject_at(seed, t, n), &p, None); }
    let t0 = Instant::now();
    for t in 0..graph_thinks { std::hint::black_box(spread::think(&live, &mut st, &mods_at(seed, t), &inject_at(seed, t, n), &p, None)); }
    let ns_graph = t0.elapsed().as_nanos() / graph_thinks as u128;

    let pk = Packed::compile(&live, &g);
    let mut sp = Activation::new(n);
    for t in 0..20 { spread::think(&pk, &mut sp, &mods_at(seed, t), &inject_at(seed, t, n), &p, None); }
    let t0 = Instant::now();
    for t in 0..graph_thinks { std::hint::black_box(spread::think(&pk, &mut sp, &mods_at(seed, t), &inject_at(seed, t, n), &p, None)); }
    let ns_packed = t0.elapsed().as_nanos() / graph_thinks as u128;
    let half = Hd::compile(&live, &g, bits / 2, seed);

    let mut sh = Activation::new(n);
    let t0 = Instant::now();
    for t in 0..hd_thinks { std::hint::black_box(spread::think(&hd, &mut sh, &mods_at(seed, t), &inject_at(seed, t, n), &p, None)); }
    let ns_hd = t0.elapsed().as_nanos() / hd_thinks.max(1) as u128;

    // Agreement: each form from empty, same inputs, tick by tick, against the graph form.
    let agree = |form: &dyn Fn(&mut Activation, &Mods, &[(u32, i32)]) -> Vec<u32>| {
        let (mut a, mut b) = (Activation::new(n), Activation::new(n));
        let (mut same, mut overlap) = (0, 0);
        for t in 0..compare {
            let (m, inj) = (mods_at(seed ^ 77, t), inject_at(seed ^ 77, t, n));
            let wa = spread::think(&live, &mut a, &m, &inj, &p, None);
            let wb = form(&mut b, &m, &inj);
            if wa.first() == wb.first() { same += 1; }
            overlap += wa.iter().filter(|x| wb.contains(x)).count() * 1000 / wa.len().max(1);
        }
        (same, overlap / compare.max(1) as usize)
    };
    let (same, overlap) = agree(&|st, m, inj| spread::think(&hd, st, m, inj, &p, None));
    let (half_same, half_overlap) = agree(&|st, m, inj| spread::think(&half, st, m, inj, &p, None));
    let (pk_same, _) = agree(&|st, m, inj| spread::think(&pk, st, m, inj, &p, None));
    Row {
        nodes: n, links: g.link_count(), graph_bytes: g.bytes() + fast.len() * 2, hd_bytes: hd.bytes(),
        codebook_bytes: hd.codebook_bytes(), state_bytes: st.bytes(), ns_graph, ns_hd, compile_ms,
        packed_bytes: pk.bytes(), ns_packed, hd_half_bytes: half.bytes(), half_same_top1: half_same,
        half_ws_overlap_pm: half_overlap, packed_same_top1: pk_same,
        compared: compare as usize, same_top1: same, ws_overlap_pm: overlap,
    }
}
