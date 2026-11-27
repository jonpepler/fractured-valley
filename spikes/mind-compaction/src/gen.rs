//! The harness: structured, realistic minds and a fixed, seeded workload of thinks.
//!
//! **A species base graph** (`species`), shared by every individual of a kind:
//! - eight core nodes: three body states (hunger, fatigue, pain) and five concepts (food, danger,
//!   rest, anger, comfort), with the body → want links the fox has;
//! - the rest in **communities** of 16–256 nodes (sizes skewed small), each a neighbourhood on a
//!   2D map: about 8% places, 12% things placed at one of the community's places, 80% concepts;
//! - **power-law out-degree** (P(deg ≥ d) ≈ (5/d)², 2..400, mean ≈ 10) and preferential in-links
//!   (low indices in a community are its hubs);
//! - link targets: 72% own community, 12% a neighbouring community on the map, 4% the core
//!   concept the node's tag points at, 12% anywhere (long-range associations);
//! - skewed weights (many weak links, few strong ones; median ≈ 260 of 1000);
//! - **anchored places with distance links**: each place links `near` to its 6 nearest places
//!   within 64 cells, weighted by real distance; things link `at` their place and the place
//!   `holds` them;
//! - 6% of links carry a link valence (`feels`), as sleep leaves them in the fox.
//!
//! **An individual** is the base plus a small **overlay** (`Overlay`): reweighted links, added
//! links, revalued nodes and a few new personal nodes. Experience is local: 70% of an overlay's
//! changes fall in the individual's home range (3% of communities), the rest anywhere.
//!
//! **The workload** (`workload`): a creature walks between places and rests at them. Each tick
//! injects what it senses (its place and the things there, glimpses while walking), its needs
//! (hunger and fatigue, felt in steps of 50), varies the modulators, and now and then a surprise
//! (a danger, an interrupt that damps the carried thought, arousal and stress up).
//!
//! Everything is integer and comes from `hash(seed, …)`.

use crate::graph::{lt, tag, Builder, Graph};
use crate::rng::hash;
use crate::spread::Mods;

pub const HUNGER: u32 = 0;
pub const FATIGUE: u32 = 1;
pub const PAIN: u32 = 2;
pub const FOOD: u32 = 3;
pub const DANGER: u32 = 4;
pub const REST: u32 = 5;
pub const ANGER: u32 = 6;
pub const COMFORT: u32 = 7;
pub const CORE: u32 = 8;

/// What the generator knows about a mind besides its graph.
#[derive(Clone, Debug, Default)]
pub struct Info {
    /// Community of each node (the core nodes are community 0). The "oracle" clustering.
    pub community: Vec<u32>,
    /// First node of each community, plus one past the end.
    pub comm_start: Vec<u32>,
    pub places: Vec<u32>,
    pub danger_nodes: Vec<u32>,
    /// Communities per side of the map grid.
    pub side: u32,
}

/// A link type guessed from what the two ends are (as in the thought graph's `hd::infer_type`).
pub fn infer_type(tu: u8, tv: u8) -> u8 {
    if tu & tag::BODY != 0 { lt::WANTS }
    else if tu & tag::PLACE != 0 && tv & tag::PLACE != 0 { lt::NEAR }
    else if tu & tag::PLACE != 0 { if tv & tag::ANCHORED != 0 { lt::HOLDS } else { lt::FEELS } }
    else if tv & tag::PLACE != 0 { lt::AT }
    else { lt::KIND }
}

fn power_degree(r: u64) -> u64 {
    let u = 1 + r % (1 << 20);
    (5 * 1024 / u.isqrt().max(1)).clamp(2, 400)
}
/// A skewed weight, 20..1000: many weak links, few strong ones.
fn skewed_weight(r: u64) -> i32 { let x = (r % 1000) as i32; 20 + x * x / 1031 }

/// The species base graph of about `n` nodes.
pub fn species(n: usize, seed: u64) -> (Graph, Info) {
    let h = |i: u64, salt: u64| hash(seed, i, salt, 0x5bec_1e5);
    let mut b = Builder::default();
    let core: [(i32, u8); 8] = [(0, tag::BODY), (0, tag::BODY), (-600, tag::BODY), (300, tag::FOOD), (-800, tag::DANGER), (200, tag::REST), (-200, tag::ANGER), (500, 0)];
    for &(v, t) in &core { b.node(v, t, None); }
    // Communities.
    let mut comm_start = vec![0u32, CORE];
    while (*comm_start.last().unwrap() as usize) < n {
        let c = comm_start.len() as u64;
        let (r1, r2) = (h(c, 1) % 241, h(c, 2) % 241);
        let size = 16 + r1 * r2 / 240;
        let next = (*comm_start.last().unwrap() as u64 + size).min(n as u64) as u32;
        comm_start.push(next);
    }
    let comms = comm_start.len() - 1; // community 0 is the core
    let side = ((comms as u64).isqrt() + 1) as u32;
    let centre = |c: u32| -> (i32, i32) { ((c % side) as i32 * 48, (c / side) as i32 * 48) };
    let mut community = vec![0u32; n];
    let mut info = Info { side, ..Default::default() };
    // Nodes, community by community: places first get positions, things go to a place.
    for c in 1..comms as u32 {
        let (s, e) = (comm_start[c as usize], comm_start[c as usize + 1]);
        let (cx, cy) = centre(c);
        let mut cplaces: Vec<(i16, i16)> = vec![];
        for i in s..e {
            let i64_ = i as u64;
            community[i as usize] = c;
            let r = h(i64_, 3) % 100;
            let t = h(i64_, 4) % 100;
            let mut tags = if t < 10 { tag::FOOD } else if t < 15 { tag::DANGER } else if t < 20 { tag::REST } else { 0 };
            let mut val = if h(i64_, 5) % 100 < 40 { (h(i64_, 6) % 1401) as i32 - 700 } else { 0 };
            if tags & tag::DANGER != 0 { val = -200 - (h(i64_, 6) % 700) as i32; }
            if tags & tag::FOOD != 0 { val = 100 + (h(i64_, 6) % 500) as i32; }
            let anchor = if r < 8 || (r < 20 && cplaces.is_empty()) {
                tags |= tag::PLACE;
                let p = ((cx + (h(i64_, 7) % 40) as i32) as i16, (cy + (h(i64_, 8) % 40) as i32) as i16);
                cplaces.push(p);
                Some(p)
            } else if r < 20 {
                if r < 12 { tags |= tag::CREATURE; }
                let p = cplaces[(h(i64_, 9) % cplaces.len() as u64) as usize];
                Some(p)
            } else { None };
            let id = b.node(val, tags, anchor);
            if tags & tag::PLACE != 0 { info.places.push(id); }
            if tags & tag::DANGER != 0 { info.danger_nodes.push(id); }
        }
    }
    let n = b.val.len();
    let tags = b.tags.clone();
    // Associative links.
    let comm_size = |c: u32| comm_start[c as usize + 1] - comm_start[c as usize];
    let pref = |c: u32, r: u64| -> u32 { let s = comm_size(c) as u64; let q = r % s; comm_start[c as usize] + (q * q / s) as u32 };
    for i in CORE as u64..n as u64 {
        let c = community[i as usize];
        let deg = power_degree(h(i, 10));
        for k in 0..deg {
            let r = h(i, 1000 + k) % 100;
            let pick = h(i, 2000 + k);
            let t = if r < 72 { pref(c, pick) }
                else if r < 84 {
                    let nb = [c as i64 + 1, c as i64 - 1, c as i64 + side as i64, c as i64 - side as i64][(pick % 4) as usize];
                    let nb = if nb >= 1 && (nb as usize) < comms { nb as u32 } else { c };
                    pref(nb, pick >> 8)
                }
                else if r < 88 {
                    let ti = tags[i as usize];
                    if ti & tag::FOOD != 0 { FOOD } else if ti & tag::DANGER != 0 { DANGER } else if ti & tag::REST != 0 { REST } else { COMFORT - (pick % 2) as u32 * 4 }
                }
                else { (pick % n as u64) as u32 };
            if t as u64 == i { continue; }
            let (ti, tt) = (tags[i as usize], tags[t as usize]);
            let mut ty = infer_type(ti, tt);
            if ty == lt::KIND && h(i, 3000 + k) % 3 == 0 { ty = lt::ASSOC; }
            let mut v = 0;
            if h(i, 4000 + k) % 100 < 6 { ty = lt::FEELS; v = (h(i, 5000 + k) % 1601) as i32 - 800; }
            b.link(i as u32, t, ty, skewed_weight(h(i, 6000 + k)), v);
        }
    }
    // Places: near links by real distance, to the 6 nearest within 64 cells (own and adjacent
    // communities); things at their place.
    let pos: Vec<(i32, i32)> = (0..n as u32).map(|i| b.anchors.binary_search_by_key(&i, |a| a.0).ok().map(|k| (b.anchors[k].1 as i32, b.anchors[k].2 as i32)).unwrap_or((0, 0))).collect();
    let mut by_comm: Vec<Vec<u32>> = vec![vec![]; comms];
    for &p in &info.places { by_comm[community[p as usize] as usize].push(p); }
    for &p in &info.places {
        let c = community[p as usize] as i64;
        let mut cand: Vec<(i32, u32)> = vec![];
        for nb in [c, c - 1, c + 1, c - side as i64, c + side as i64] {
            if nb < 1 || nb as usize >= comms { continue; }
            for &q in &by_comm[nb as usize] {
                if q == p { continue; }
                let (dx, dy) = (pos[p as usize].0 - pos[q as usize].0, pos[p as usize].1 - pos[q as usize].1);
                let d = ((dx * dx + dy * dy) as u32).isqrt() as i32;
                if d <= 64 { cand.push((d, q)); }
            }
        }
        cand.sort_unstable();
        for &(d, q) in cand.iter().take(6) { b.link(p, q, lt::NEAR, 1000 - d * 1000 / 66, 0); }
        // Like: the next place in the community with the same kind.
        let pt = tags[p as usize] & (tag::FOOD | tag::REST | tag::DANGER);
        if pt != 0 {
            if let Some(&q) = by_comm[c as usize].iter().find(|&&q| q > p && tags[q as usize] & pt != 0) { b.link(p, q, lt::LIKE, 300, 0); b.link(q, p, lt::LIKE, 300, 0); }
        }
    }
    for i in CORE..n as u32 {
        if tags[i as usize] & tag::ANCHORED == 0 || tags[i as usize] & tag::PLACE != 0 { continue; }
        let c = community[i as usize] as usize;
        if let Some(&pl) = by_comm[c].iter().find(|&&q| pos[q as usize] == pos[i as usize]) {
            b.link(i, pl, lt::AT, 800, 0);
            b.link(pl, i, lt::HOLDS, 600, 0);
        }
    }
    // Core: body wants, concept hubs.
    let pick_tagged = |want: u8, salt: u64, k: usize| -> Vec<u32> {
        (0..k as u64 * 8).map(|j| (CORE as u64 + h(j, salt) % (n as u64 - CORE as u64)) as u32).filter(|&v| tags[v as usize] & want != 0).take(k).collect()
    };
    b.link(HUNGER, FOOD, lt::WANTS, 900, 0);
    b.link(FATIGUE, REST, lt::WANTS, 900, 0);
    b.link(PAIN, DANGER, lt::WANTS, 800, 0);
    b.link(PAIN, ANGER, lt::WANTS, 300, 0);
    b.link(DANGER, ANGER, lt::KIND, 400, 0);
    b.link(REST, COMFORT, lt::KIND, 500, 0);
    for v in pick_tagged(tag::FOOD, 7001, 16) { b.link(HUNGER, v, lt::WANTS, 400, 0); }
    for v in pick_tagged(tag::REST, 7002, 16) { b.link(FATIGUE, v, lt::WANTS, 400, 0); }
    for (hub, want, salt) in [(FOOD, tag::FOOD, 7003), (DANGER, tag::DANGER, 7004), (REST, tag::REST, 7005)] {
        for v in pick_tagged(want, salt, 64) { b.link(hub, v, lt::KIND, 300, 0); }
    }
    info.community = community;
    info.comm_start = comm_start;
    (b.build(), info)
}

/// How much an individual differs from its species, per mille of links or nodes.
#[derive(Clone, Copy, Debug)]
pub struct OverlaySpec { pub reweight_pm: u32, pub add_pm: u32, pub revalue_pm: u32, pub new_node_pm: u32 }
impl OverlaySpec {
    /// The default individual: 2% of links reweighted, 1% added, 1% of nodes revalued, 0.5% new.
    pub const DEFAULT: OverlaySpec = OverlaySpec { reweight_pm: 20, add_pm: 10, revalue_pm: 10, new_node_pm: 5 };
    pub fn scaled(self, num: u32, den: u32) -> OverlaySpec {
        OverlaySpec { reweight_pm: self.reweight_pm * num / den, add_pm: self.add_pm * num / den, revalue_pm: self.revalue_pm * num / den, new_node_pm: self.new_node_pm * num / den }
    }
}

/// An individual's difference from its species base. Every list is sorted, so it is canonical.
#[derive(Clone, Debug, Default, Hash)]
pub struct Overlay {
    /// (base link index, new weight).
    pub reweight: Vec<(u32, i16)>,
    /// Added links (source, target, type, weight, valence), sorted by (source, target, type).
    pub added: Vec<(u32, u32, u8, i16, i16)>,
    /// (node, new valence).
    pub revalue: Vec<(u32, i16)>,
    /// New personal nodes: valence, tags, anchor. Their ids follow the base's.
    pub new_nodes: Vec<(i16, u8, Option<(i16, i16)>)>,
}

impl Overlay {
    /// Bytes as stored: 6 per reweight and revalue, 13 per added link, 3 (+ anchor) per new node.
    pub fn bytes(&self) -> usize {
        self.reweight.len() * 6 + self.added.len() * 13 + self.revalue.len() * 6
            + self.new_nodes.iter().map(|n| 3 + if n.2.is_some() { 12 } else { 0 }).sum::<usize>()
    }
}

/// The overlay for individual `who` of a species.
pub fn overlay(base: &Graph, info: &Info, spec: OverlaySpec, seed: u64, who: u32) -> Overlay {
    let h = |i: u64, salt: u64| hash(seed, who as u64, i, salt);
    let n = base.len() as u64;
    let comms = info.comm_start.len() as u64 - 1;
    let home: Vec<u32> = (0..(comms * 3 / 100).max(1)).map(|k| 1 + (h(k, 1) % (comms - 1).max(1)) as u32).collect();
    // A node: 70% from the home range, 30% anywhere.
    let node = |r: u64| -> u32 {
        if r % 10 < 7 {
            let c = home[((r >> 8) % home.len() as u64) as usize] as usize;
            let (s, e) = (info.comm_start[c] as u64, info.comm_start[c + 1] as u64);
            (s + (r >> 24) % (e - s).max(1)) as u32
        } else { (CORE as u64 + (r >> 8) % (n - CORE as u64)) as u32 }
    };
    let links = base.link_count() as u64;
    let mut ov = Overlay::default();
    // Reweights: a random link of a (mostly home-range) node.
    let k = links * spec.reweight_pm as u64 / 1000;
    for j in 0..k {
        let u = node(h(j, 2));
        let r = base.links(u);
        if r.is_empty() { continue; }
        let l = r.start + (h(j, 3) % r.len() as u64) as usize;
        ov.reweight.push((l as u32, skewed_weight(h(j, 4)) as i16));
    }
    ov.reweight.sort_unstable();
    ov.reweight.dedup_by_key(|x| x.0);
    // New nodes, then added links (some from and to the new nodes).
    let nn = n * spec.new_node_pm as u64 / 1000;
    for j in 0..nn {
        let near = node(h(j, 5));
        let t = base.tags[near as usize] & (tag::FOOD | tag::DANGER | tag::REST);
        let anchor = base.anchor(near);
        ov.new_nodes.push(((h(j, 6) % 1401) as i16 - 700, t | if anchor.is_some() { tag::ANCHORED } else { 0 }, anchor));
    }
    let tag_of = |v: u32| -> u8 { if (v as u64) < n { base.tags[v as usize] } else { ov.new_nodes[(v as u64 - n) as usize].1 } };
    let mut added = vec![];
    let ka = links * spec.add_pm as u64 / 1000;
    for j in 0..ka {
        let r = h(j, 7);
        let (s, t) = if nn > 0 && r % 4 == 0 {
            // A link from or to a new node, to something in its neighbourhood.
            let nv = n as u32 + (h(j, 8) % nn) as u32;
            let other = node(h(j, 9));
            if r % 8 == 0 { (nv, other) } else { (other, nv) }
        } else {
            let s = node(h(j, 10));
            let c = info.community[s as usize] as usize;
            let (cs, ce) = (info.comm_start[c] as u64, info.comm_start[c + 1] as u64);
            (s, (cs + h(j, 11) % (ce - cs).max(1)) as u32)
        };
        if s == t { continue; }
        let ty = infer_type(tag_of(s), tag_of(t));
        if (s as u64) < n && (t as u64) < n && base.find(s, t, ty).is_some() { continue; }
        let v = if h(j, 12) % 100 < 20 { (h(j, 13) % 1601) as i32 - 800 } else { 0 };
        added.push((s, t, ty, skewed_weight(h(j, 14)) as i16, v as i16));
    }
    added.sort_unstable_by_key(|l: &(u32, u32, u8, i16, i16)| (l.0, l.1, l.2, std::cmp::Reverse(l.3), l.4));
    added.dedup_by_key(|l| (l.0, l.1, l.2));
    ov.added = added;
    let kv = n * spec.revalue_pm as u64 / 1000;
    for j in 0..kv {
        let u = node(h(j, 15));
        ov.revalue.push((u, ((base.val[u as usize] as i32 + (h(j, 16) % 801) as i32 - 400).clamp(-1000, 1000)) as i16));
    }
    ov.revalue.sort_unstable();
    ov.revalue.dedup_by_key(|x| x.0);
    ov
}

/// The individual's whole graph: base plus overlay, materialised.
pub fn materialise(base: &Graph, ov: &Overlay) -> Graph {
    let mut b = Builder::default();
    let mut rv = ov.revalue.iter().peekable();
    for i in 0..base.len() as u32 {
        let mut v = base.val[i as usize];
        if rv.peek().is_some_and(|x| x.0 == i) { v = rv.next().unwrap().1; }
        b.node(v as i32, base.tags[i as usize] & !tag::ANCHORED, base.anchor(i));
    }
    for &(v, t, a) in &ov.new_nodes { b.node(v as i32, t & !tag::ANCHORED, a); }
    let mut links = base.link_tuples();
    for &(l, w) in &ov.reweight { links[l as usize].3 = w; }
    links.extend_from_slice(&ov.added);
    b.links = links;
    b.build()
}

/// One tick of the workload.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Tick {
    pub mods: Mods,
    pub inject: Vec<(u32, i32)>,
    /// Per mille of the carried thought kept before thinking (1000: no interrupt).
    pub damp: i32,
}

/// A fixed, seeded workload of `ticks` thinks over mind `g`.
pub fn workload(g: &Graph, info: &Info, seed: u64, ticks: usize) -> Vec<Tick> {
    let h = |t: u64, salt: u64| hash(seed, t, salt, 0x770_4c10ad);
    let places = &info.places;
    let mut here = places[(h(0, 1) % places.len() as u64) as usize];
    let mut dest: Option<(u32, u32)> = None; // (place, ticks left)
    let mut rest_left = 10u32;
    let (mut hunger, mut fatigue, mut arousal, mut mood, mut stress) = (400i32, 200i32, 0i32, 0i32, 0i32);
    let holds = |p: u32| -> Vec<(u32, i32)> {
        g.links(p).filter(|&l| g.ltype[l] == lt::HOLDS).take(3).map(|l| (g.targets[l], 200 + g.slow[l] as i32 / 2)).collect()
    };
    let near = |p: u32, r: u64| -> u32 {
        let ns: Vec<u32> = g.links(p).filter(|&l| g.ltype[l] == lt::NEAR).map(|l| g.targets[l]).collect();
        if ns.is_empty() { places[(r % places.len() as u64) as usize] } else { ns[(r % ns.len() as u64) as usize] }
    };
    let mut out = Vec::with_capacity(ticks);
    for t in 0..ticks as u64 {
        let mut inject: Vec<(u32, i32)> = vec![];
        let mut damp = 1000;
        match dest {
            Some((q, left)) => {
                // Walking: the place behind, then the place ahead, and a glimpse of something.
                let at = if left > 4 { here } else { q };
                inject.push((at, 250));
                let c = info.community[at as usize] as usize;
                let (cs, ce) = (info.comm_start[c] as u64, info.comm_start[c + 1] as u64);
                inject.push(((cs + h(t, 2) % (ce - cs)) as u32, 300 + (h(t, 3) % 300) as i32));
                fatigue = (fatigue + 3).min(1000);
                if left <= 1 { here = q; dest = None; rest_left = 5 + (h(t, 4) % 40) as u32; } else { dest = Some((q, left - 1)); }
            }
            None => {
                // Resting: the same place and things each tick.
                inject.push((here, 300));
                let things = holds(here);
                if g.tags[here as usize] & tag::FOOD != 0 || things.iter().any(|&(x, _)| g.tags[x as usize] & tag::FOOD != 0) { hunger = (hunger - 30).max(0); }
                inject.extend(things);
                fatigue = (fatigue - if g.tags[here as usize] & tag::REST != 0 { 12 } else { 4 }).max(0);
                if rest_left == 0 { dest = Some((near(here, h(t, 5)), 6 + (h(t, 6) % 10) as u32)); } else { rest_left -= 1; }
            }
        }
        hunger = (hunger + 2).min(1000);
        // Needs, felt in steps of 50.
        if hunger > 300 { inject.push((HUNGER, hunger / 50 * 50 * 2 / 3)); }
        if fatigue > 400 { inject.push((FATIGUE, fatigue / 50 * 50 / 2)); }
        // A surprise, now and then.
        if h(t, 7) % 200 == 0 && !info.danger_nodes.is_empty() {
            let d = info.danger_nodes[(h(t, 8) % info.danger_nodes.len() as u64) as usize];
            inject.push((d, 1000));
            inject.push((DANGER, 600));
            if h(t, 9) % 3 == 0 { inject.push((PAIN, 700)); }
            damp = 300;
            arousal = (arousal + 500).min(1000);
            stress = (stress + 400).min(1000);
        }
        arousal = arousal * 15 / 16;
        stress = stress * 39 / 40;
        mood = (mood + (h(t, 10) % 21) as i32 - 10 - mood / 64).clamp(-800, 800);
        out.push(Tick { mods: Mods { hunger, arousal, mood, fatigue, stress }, inject, damp });
    }
    out
}
