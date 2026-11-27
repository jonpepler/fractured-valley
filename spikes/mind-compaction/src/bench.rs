//! The lab: build a mind, replay the workload through every variant, compare with the
//! reference tick by tick, and collect rows.

use crate::decide::{pick, score_of, STAY, WALK_AT};
use crate::forms::{self, Aos, Codec, Compact, Cow, Hd, Packed, Quant};
use crate::gen::{self, Info, Overlay, OverlaySpec, Tick};
use crate::graph::Graph;
use crate::perf::Counter;
use crate::rng::StateHasher;
use crate::think2::{GainMode, Opts, Select};
use crate::variants::{Delta, Hier, Mode, Skip, Std, Swap, Variant};
use std::hash::Hasher;
use std::time::Instant;

pub const SEED: u64 = 0x00f0_c0de_2026_1127;
pub const TICKS: usize = 1500;

/// One individual: its species base, its overlay, the materialised mind, and its workload.
pub struct Lab {
    pub n: usize,
    pub base: Graph,
    pub info: Info,
    pub ov: Overlay,
    pub g: Graph,
    /// Cluster of every node of `g` (the generator's communities; new nodes join their first
    /// link's community).
    pub cluster: Vec<u32>,
    pub ticks: Vec<Tick>,
    pub gen_ms: u128,
}

pub fn lab(n: usize, seed: u64, ticks: usize) -> Lab {
    let t0 = Instant::now();
    let (base, info) = gen::species(n, seed);
    let ov = gen::overlay(&base, &info, OverlaySpec::DEFAULT, seed, 1);
    let g = gen::materialise(&base, &ov);
    let mut cluster = info.community.clone();
    for v in base.len()..g.len() {
        let c = g.links(v as u32).next().map(|l| g.targets[l] as usize).filter(|&t| t < base.len()).map(|t| info.community[t]).unwrap_or(0);
        cluster.push(c);
    }
    let ticks = gen::workload(&g, &info, seed, ticks);
    Lab { n, base, info, ov, g, cluster, ticks, gen_ms: t0.elapsed().as_millis() }
}

/// Every variant the lab knows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Reference,
    Packed,
    Hd(usize),
    Swap,
    Cow(bool),
    Quant(Codec),
    Prune(i16),
    TopK(usize),
    Hier(usize),
    Skip(i32),
    Delta,
    Think(Select, bool, bool, GainMode),
    Fused(bool),
    Aos,
    Shuffled,
    ShuffledBfs,
    ShuffledRcm,
    CompactNatural,
    Compact8,
    CompactRcm,
    Stack,
    Stack8,
}

/// How faithful a variant is meant to be.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Faith {
    /// Bit-identical to the reference (same run hash).
    Exact,
    /// Identical activations; may break ties differently (a renumbered mind).
    UpToTies,
    /// Lossy but meant to stay faithful: held to the agreement floor in the tests.
    Floor,
    /// Lossy on purpose; measured, not held to a floor.
    Explore,
}

pub struct Spec { pub kind: Kind, pub name: String, pub family: &'static str, pub faith: Faith, pub max_n: usize, pub ticks: usize }

pub fn specs() -> Vec<Spec> {
    let s = |kind, name: &str, family, faith| Spec { kind, name: name.into(), family, faith, max_n: usize::MAX, ticks: TICKS };
    let mut v = vec![
        s(Kind::Reference, "reference graph", "baseline", Faith::Exact),
        s(Kind::Packed, "packed (u8 weight, no link valence)", "baseline", Faith::Floor),
        Spec { kind: Kind::Hd(2048), name: "bit vectors 2048 (whole mind)".into(), family: "baseline", faith: Faith::Explore, max_n: 10_000, ticks: 60 },
        s(Kind::Swap, "swap-in state (active list only)", "state", Faith::Exact),
        s(Kind::Cow(false), "shared base + overlay (COW)", "sharing", Faith::Exact),
        s(Kind::Cow(true), "COW, dense per-mind valence", "sharing", Faith::Exact),
    ];
    for c in [Codec::U16, Codec::U8, Codec::U4Lin, Codec::U4Log, Codec::U3Log, Codec::U2Log] {
        let faith = if matches!(c, Codec::U16 | Codec::U8) { Faith::Floor } else { Faith::Explore };
        v.push(s(Kind::Quant(c), &format!("weights {}", c.name()), "precision", faith));
    }
    for w in [50, 100, 200] { v.push(s(Kind::Prune(w), &format!("prune links < {w}"), "pruning", Faith::Explore)); }
    for k in [16, 8, 4] { v.push(s(Kind::TopK(k), &format!("top {k} links per node"), "pruning", Faith::Explore)); }
    for k in [4, 16, 64] { v.push(s(Kind::Hier(k), &format!("hierarchical, {k} clusters"), "hierarchy", Faith::Explore)); }
    for t in [0, 20, 60] { v.push(s(Kind::Skip(t), &format!("skip unchanged (tolerance {t})"), "event", Faith::Explore)); }
    v.push(s(Kind::Delta, "delta input (steady input not re-spread)", "event", Faith::Explore));
    v.push(s(Kind::Think(Select::Nth, false, false, GainMode::Full), "frontier: select_nth", "think", Faith::Exact));
    v.push(s(Kind::Think(Select::Heap, false, false, GainMode::Full), "frontier: fixed-size heap", "think", Faith::Exact));
    v.push(s(Kind::Think(Select::Nth, true, false, GainMode::Full), "select_nth + gain memo", "think", Faith::Exact));
    v.push(s(Kind::Think(Select::Nth, false, false, GainMode::Table), "select_nth + two-stage gain table", "think", Faith::Exact));
    v.push(s(Kind::Think(Select::Nth, false, true, GainMode::Full), "select_nth + fixed-point flow", "think", Faith::Floor));
    v.push(s(Kind::Think(Select::Nth, false, true, GainMode::Approx), "select_nth + fixed-point flow and gain", "think", Faith::Floor));
    v.push(s(Kind::Fused(false), "fused think (exact)", "think", Faith::Exact));
    v.push(s(Kind::Fused(true), "fused think, fixed-point gain", "think", Faith::Floor));
    v.push(s(Kind::Aos, "array of structs links", "layout", Faith::Exact));
    v.push(s(Kind::Shuffled, "shuffled ids (learning order)", "layout", Faith::UpToTies));
    v.push(s(Kind::ShuffledBfs, "shuffled, then BFS order", "layout", Faith::UpToTies));
    v.push(s(Kind::ShuffledRcm, "shuffled, then RCM order", "layout", Faith::UpToTies));
    v.push(s(Kind::CompactNatural, "compact: gap-coded, 4-bit log", "compact", Faith::Explore));
    v.push(s(Kind::CompactRcm, "compact 4-bit, on RCM order", "compact", Faith::Explore));
    v.push(s(Kind::Compact8, "compact: gap-coded, u8 weights", "compact", Faith::Floor));
    v.push(s(Kind::Stack, "stack: compact 4-bit + swap + fixed-point", "compact", Faith::Explore));
    v.push(s(Kind::Stack8, "stack: compact u8 + swap + fixed-point", "compact", Faith::Floor));
    v
}

fn std_graph(g: Graph) -> Std<Graph> { let b = g.bytes(); Std::new(g, b) }

/// Build a variant over the lab's mind.
pub fn build<'a>(kind: Kind, lab: &'a Lab) -> Box<dyn Variant + 'a> {
    let g = &lab.g;
    match kind {
        Kind::Reference => Box::new(Std::new(g, g.bytes())),
        Kind::Packed => { let p = Packed::compile(g); let b = p.bytes(); Box::new(Std::new(p, b)) }
        Kind::Hd(bits) => { let h = Hd::compile(g, bits, SEED); let b = h.bytes(); let cb = h.codebook.len() * 8; Box::new(Std::new(h, b).shared(cb)) }
        Kind::Swap => Box::new(Swap { inner: Std::new(g, g.bytes()), saved: vec![] }),
        Kind::Cow(d) => { let c = Cow::new(&lab.base, &lab.ov); let c = if d { c.dense() } else { c }; let b = c.bytes(); Box::new(Std::new(c, b).shared(lab.base.bytes())) }
        Kind::Quant(c) => { let q = Quant::compile(g, c); let b = q.bytes(); Box::new(Std::new(q, b)) }
        Kind::Prune(w) => Box::new(std_graph(forms::prune(g, w, 0))),
        Kind::TopK(k) => Box::new(std_graph(forms::prune(g, 0, k))),
        Kind::Hier(k) => Box::new(Hier::new(g, &lab.cluster, k)),
        Kind::Skip(t) => Box::new(Skip::new(Std::new(g, g.bytes()), t)),
        Kind::Delta => Box::new(Delta::new(Std::new(g, g.bytes()))),
        Kind::Think(sel, memo, fixed, gain) => Box::new(Std::new(g, g.bytes()).mode(Mode::Opt(Opts { select: sel, memo, fixed, gain }))),
        Kind::Fused(a) => Box::new(crate::fused::FusedMind::new(g, a)),
        Kind::Aos => { let a = Aos::compile(g); let b = a.bytes(); Box::new(Std::new(a, b)) }
        Kind::Shuffled | Kind::ShuffledBfs | Kind::ShuffledRcm => {
            let s = forms::shuffled(g.len(), SEED);
            let gs = g.permute(&s);
            let (gg, perm) = match kind {
                Kind::Shuffled => (gs, s),
                _ => {
                    let r = if kind == Kind::ShuffledBfs { forms::bfs(&gs) } else { forms::rcm(&gs) };
                    let total: Vec<u32> = s.iter().map(|&x| r[x as usize]).collect();
                    (gs.permute(&r), total)
                }
            };
            Box::new(std_graph(gg).perm(perm))
        }
        Kind::CompactNatural => { let c = Compact::compile(g); let b = c.bytes(); Box::new(Std::new(c, b)) }
        Kind::Compact8 => { let c = Compact::compile_with(g, true); let b = c.bytes(); Box::new(Std::new(c, b)) }
        Kind::Stack | Kind::Stack8 => {
            let c = Compact::compile_with(g, kind == Kind::Stack8);
            let b = c.bytes();
            let inner = Std::new(c, b).mode(Mode::Opt(Opts { select: Select::Nth, memo: false, fixed: true, gain: GainMode::Approx }));
            Box::new(Swap { inner, saved: vec![] })
        }
        Kind::CompactRcm => { let r = forms::rcm(g); let c = Compact::compile(&g.permute(&r)); let b = c.bytes(); Box::new(Std::new(c, b).perm(r)) }
    }
}

/// What a run produced, in reference node ids.
pub struct Run {
    pub hash: u64,
    pub ns: Vec<u64>,
    pub visits: u64,
    pub ws: Vec<Vec<u32>>,
    pub act: Vec<Vec<(u32, i32)>>,
    pub dec: Vec<u32>,
    /// The decision rule's scores per tick (reference ids), for tie-aware agreement.
    pub scores: Vec<Vec<(u32, i32)>>,
    pub misses: Option<u64>,
}

/// Measured passes per variant. Each tick's time is the minimum over the passes (robust to
/// other work on the machine); every pass must give the same hash.
pub const PASSES: usize = 5;

/// Replay `ticks` through a variant: once to warm up, then `PASSES` times measured, resetting
/// the mind before each. Panics if two passes disagree (a determinism failure).
pub fn run(v: &mut dyn Variant, ticks: &[Tick]) -> Run {
    let perm = v.perm().map(|p| p.to_vec());
    let inv = perm.as_ref().map(|p| { let mut inv = vec![0u32; p.len()]; for (i, &x) in p.iter().enumerate() { inv[x as usize] = i as u32; } inv });
    let mapped: Vec<Tick> = match &perm {
        Some(p) => ticks.iter().map(|t| Tick { mods: t.mods, damp: t.damp, inject: t.inject.iter().map(|&(n, a)| (p[n as usize], a)).collect() }).collect(),
        None => ticks.to_vec(),
    };
    let back = |n: u32| -> u32 { match &inv { Some(i) => i[n as usize], None => n } };
    for t in &mapped { std::hint::black_box(v.think(t)); }
    let counter = Counter::cache_misses();
    let mut best: Option<Run> = None;
    for _pass in 0..PASSES {
        v.reset();
        let mut out = Run { hash: 0, ns: Vec::with_capacity(ticks.len()), visits: 0, ws: vec![], act: vec![], dec: vec![], scores: vec![], misses: None };
        let mut h = StateHasher::new(SEED);
        let mut misses = 0u64;
        for t in &mapped {
            if let Some(c) = &counter { c.start(); }
            let t0 = Instant::now();
            let ws = v.think(t);
            let ns = t0.elapsed().as_nanos() as u64;
            if let Some(c) = &counter { misses += c.stop(); }
            out.ns.push(ns);
            out.visits += v.visits() as u64;
            let sc = v.scores(&ws, &t.mods);
            let d = pick(&sc);
            let d = if d == STAY { STAY } else { back(d) };
            let sc: Vec<(u32, i32)> = sc.into_iter().map(|(p, x)| (back(p), x)).collect();
            let ws: Vec<u32> = ws.iter().map(|&n| back(n)).collect();
            let mut act: Vec<(u32, i32)> = v.active().into_iter().map(|(n, a)| (back(n), a)).collect();
            act.sort_unstable();
            for &n in &ws { h.write_u32(n); }
            h.write_u32(d);
            for &(n, a) in &act { h.write_u32(n); h.write_i32(a); }
            out.ws.push(ws);
            out.act.push(act);
            out.dec.push(d);
            out.scores.push(sc);
        }
        out.hash = h.finish();
        out.misses = counter.as_ref().map(|_| misses);
        best = Some(match best {
            None => out,
            Some(mut b) => {
                assert_eq!(b.hash, out.hash, "a variant gave different results on two identical passes");
                for (x, y) in b.ns.iter_mut().zip(&out.ns) { *x = (*x).min(*y); }
                b.misses = match (b.misses, out.misses) { (Some(x), Some(y)) => Some(x.min(y)), _ => None };
                b
            }
        });
    }
    best.unwrap()
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Agree {
    pub top1_pm: u32,
    pub ws_pm: u32,
    pub action_pm: u32,
    pub act_err_pm: u32,
    /// Tie-aware: a pick counts when the reference ranks it equal to its own (same activation
    /// for workspace members and the top thought, same score for the action). Integer
    /// activations tie often, and the reference breaks ties by node id, so a renumbered or
    /// differently-rounded mind can pick a different, equally good node.
    pub top1_tie_pm: u32,
    pub ws_tie_pm: u32,
    pub action_tie_pm: u32,
    /// Decision regret: how much worse the variant's choice is under the reference's own
    /// scores, per mille of the reference's best (staying is worth the walk bar).
    pub regret_pm: u32,
}

/// Agreement of `o` with the reference `r`, over `o`'s ticks. `g` is the reference mind (for
/// scoring the variant's choice the reference's way).
pub fn agree(r: &Run, o: &Run, g: &Graph, ticks: &[Tick]) -> Agree {
    let mut edges = vec![];
    let n = o.ws.len().max(1) as u64;
    let (mut top1, mut ws, mut action, mut err) = (0u64, 0u64, 0u64, 0u64);
    let (mut top1t, mut wst, mut actiont, mut regret) = (0u64, 0u64, 0u64, 0u64);
    for t in 0..o.ws.len() {
        let (a, b) = (&r.ws[t], &o.ws[t]);
        let (x, y) = (&r.act[t], &o.act[t]);
        let ract = |n: u32| x.binary_search_by_key(&n, |e| e.0).map(|i| x[i].1).unwrap_or(0);
        if a.first() == b.first() { top1 += 1; }
        if a.first() == b.first() || (a.first().is_some() && b.first().is_some_and(|&v| ract(v) == ract(a[0]))) { top1t += 1; }
        let k = a.len().max(b.len());
        ws += if k == 0 { 1000 } else { (a.iter().filter(|v| b.contains(v)).count() * 1000 / k) as u64 };
        let edge = a.last().map(|&v| ract(v)).unwrap_or(0);
        wst += if k == 0 { 1000 } else { (b.iter().filter(|&&v| a.contains(&v) || (edge > 0 && ract(v) >= edge)).count().min(a.len()) * 1000 / k) as u64 };
        let mut value = |d: u32| -> i64 { if d == STAY { WALK_AT as i64 } else { score_of(g, &ract, a, &ticks[t].mods, d, &mut edges) as i64 } };
        let best = value(r.dec[t]).max(1);
        regret += ((best - value(o.dec[t]).min(best)) * 1000 / best) as u64;
        if r.dec[t] == o.dec[t] { action += 1; actiont += 1; } else if o.dec[t] != STAY && r.dec[t] != STAY {
            let rs = |p: u32| r.scores[t].iter().find(|s| s.0 == p).map(|s| s.1);
            if rs(o.dec[t]).is_some() && rs(o.dec[t]) == rs(r.dec[t]) { actiont += 1; }
        }
        // Activation error: Σ |a_ref − a_var| over both active sets, per mille of Σ a_ref.
        let (mut i, mut j, mut d, mut tot) = (0, 0, 0i64, 0i64);
        while i < x.len() || j < y.len() {
            if j >= y.len() || (i < x.len() && x[i].0 < y[j].0) { d += x[i].1 as i64; tot += x[i].1 as i64; i += 1; }
            else if i >= x.len() || y[j].0 < x[i].0 { d += y[j].1 as i64; j += 1; }
            else { d += (x[i].1 - y[j].1).abs() as i64; tot += x[i].1 as i64; i += 1; j += 1; }
        }
        err += if tot > 0 { (d * 1000 / tot) as u64 } else if d > 0 { 1000 } else { 0 };
    }
    let pm = |x: u64| (x * 1000 / n) as u32;
    Agree { top1_pm: pm(top1), ws_pm: (ws / n) as u32, action_pm: pm(action), act_err_pm: (err / n) as u32, top1_tie_pm: pm(top1t), ws_tie_pm: (wst / n) as u32, action_tie_pm: pm(actiont), regret_pm: (regret / n) as u32 }
}

#[derive(Clone, Debug)]
pub struct Row {
    pub n: usize,
    pub nodes: usize,
    pub links: usize,
    pub name: String,
    pub family: &'static str,
    pub faith: Faith,
    pub bytes: usize,
    pub shared: usize,
    pub state: usize,
    pub build_ms: u128,
    pub ns_mean: u64,
    pub ns_p50: u64,
    pub ns_p99: u64,
    pub misses: Option<u64>,
    pub visits: u64,
    pub agree: Agree,
    pub hash: u64,
    pub exact: bool,
    pub ticks: usize,
    pub note: String,
}

fn pct(ns: &[u64], q: usize) -> u64 { let mut v = ns.to_vec(); v.sort_unstable(); v[(v.len() * q / 100).min(v.len() - 1)] }

pub fn measure(lab: &Lab, spec: &Spec, reference: &Run) -> (Row, Run) {
    let t0 = Instant::now();
    let mut v = build(spec.kind, lab);
    let build_ms = t0.elapsed().as_millis();
    let ticks = &lab.ticks[..spec.ticks.min(lab.ticks.len())];
    let r = run(v.as_mut(), ticks);
    let a = agree(reference, &r, &lab.g, ticks);
    let mut ref_h = StateHasher::new(SEED);
    for t in 0..ticks.len() {
        for &n in &reference.ws[t] { ref_h.write_u32(n); }
        ref_h.write_u32(reference.dec[t]);
        for &(n, x) in &reference.act[t] { ref_h.write_u32(n); ref_h.write_i32(x); }
    }
    let k = ticks.len() as u64;
    let row = Row {
        n: lab.n, nodes: lab.g.len(), links: lab.g.link_count(), name: spec.name.clone(), family: spec.family, faith: spec.faith,
        bytes: v.bytes(), shared: v.shared_bytes(), state: v.state_bytes(), build_ms,
        ns_mean: r.ns.iter().sum::<u64>() / k, ns_p50: pct(&r.ns, 50), ns_p99: pct(&r.ns, 99),
        misses: r.misses.map(|m| m / k), visits: r.visits / k, agree: a, hash: r.hash, exact: r.hash == ref_h.finish(), ticks: ticks.len(), note: v.note(),
    };
    (row, r)
}

/// All variants at one size. Returns the rows and the reference run.
pub fn bench_size(lab: &Lab, specs: &[Spec], mut progress: impl FnMut(&Row)) -> (Vec<Row>, Run) {
    let mut rv = build(Kind::Reference, lab);
    let reference = run(rv.as_mut(), &lab.ticks);
    drop(rv);
    let mut rows = vec![];
    for s in specs.iter().filter(|s| lab.n <= s.max_n) {
        let (row, _) = measure(lab, s, &reference);
        progress(&row);
        rows.push(row);
    }
    (rows, reference)
}

// ---------------------------------------------------------------------------------------------
// Sharing: bytes per extra mind when many minds share one species base.

#[derive(Clone, Debug)]
pub struct ShareRow { pub n: usize, pub minds: usize, pub delta: &'static str, pub base_bytes: usize, pub overlay_mean: usize, pub overlay_max: usize, pub page_cow_mean: usize, pub independent_per_mind: usize }

pub fn sharing(lab: &Lab, minds: usize, delta: &'static str, spec: OverlaySpec) -> ShareRow {
    let threads = std::thread::available_parallelism().map(|x| x.get()).unwrap_or(1).min(8);
    let per: Vec<(usize, usize)> = std::thread::scope(|s| {
        let hs: Vec<_> = (0..threads).map(|t| s.spawn(move || {
            let mut out = vec![];
            for who in (t..minds).step_by(threads) {
                let ov = gen::overlay(&lab.base, &lab.info, spec, SEED, 1000 + who as u32);
                out.push((ov.bytes(), forms::page_cow_bytes(&lab.base, &ov, 64)));
            }
            out
        })).collect();
        hs.into_iter().flat_map(|h| h.join().unwrap()).collect()
    });
    let m = per.len().max(1);
    ShareRow {
        n: lab.n, minds, delta, base_bytes: lab.base.bytes(),
        overlay_mean: per.iter().map(|p| p.0).sum::<usize>() / m, overlay_max: per.iter().map(|p| p.0).max().unwrap_or(0),
        page_cow_mean: per.iter().map(|p| p.1).sum::<usize>() / m, independent_per_mind: lab.g.bytes(),
    }
}

/// Distinct links between co-active workspace nodes over the run: what a day's fast weights
/// would touch, if stored sparsely instead of 2 bytes on every link.
pub fn fast_weight_links(lab: &Lab, r: &Run) -> usize {
    let mut set: Vec<usize> = vec![];
    for ws in &r.ws { for &a in ws { for &b in ws { if a != b { for l in lab.g.links(a) { if lab.g.targets[l] == b { set.push(l); } } } } } }
    set.sort_unstable();
    set.dedup();
    set.len()
}
