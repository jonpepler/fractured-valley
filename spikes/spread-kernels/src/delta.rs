//! One instance's private delta over the shared graph, compiled so that a kernel never has to
//! sort, deduplicate or look up a value at run time.
//!
//! Everything an instance changes is expressed per *source* node: an instance edge differs from
//! the shared edge only by its weight and its gain key (a revalued target changes the key of
//! every edge into it, and an added edge that duplicates a shared one resolves into a
//! reweight-and-revalue of that edge, as the materialiser's dedup does). Genuinely new edges are
//! kept as extras. A source is *patched* if it has either; its denominator is stored.

use crate::shared::{gkey, PEdge, Shared};
use mc::gen::Overlay;
use mc::spread::{Edge, Form};

#[derive(Clone, Debug, Default)]
pub struct Delta {
    pub base_n: u32,
    /// Per patched source (in node order; found through `rank`): its denominator.
    pub den: Vec<i32>,
    /// Per patched source: gain classes of its positive-weight edges (as `Shared::cmask`).
    pub pmask: Vec<u32>,
    /// Per patched source: overrides ovr[ostart[i]..ostart[i+1]] and extras extra[xstart[i]..xstart[i+1]].
    pub ostart: Vec<u32>,
    pub xstart: Vec<u32>,
    /// (edge position within the source's shared list, new weight, new key), in position order.
    pub ovr: Vec<(u16, i16, u16)>,
    pub extra: Vec<PEdge>,
    /// Rank directory over the patched sources: per 64 nodes, [mask lo, mask hi, index of the block's first patched source].
    pub rank: Vec<[u32; 3]>,
    /// Values and tags of revalued and new nodes, for the `Form` view.
    pub reval: Vec<(u32, i16)>,
    pub new_val: Vec<i16>,
    pub new_tags: Vec<u8>,
}

impl Delta {
    pub fn new(sh: &Shared, ov: &Overlay) -> Delta {
        let n = sh.n as u32;
        let mut reval = ov.revalue.clone(); reval.sort_unstable();
        let new_val: Vec<i16> = ov.new_nodes.iter().map(|x| x.0).collect();
        let new_tags: Vec<u8> = ov.new_nodes.iter().map(|x| x.1 & !mc::graph::tag::ANCHORED).collect();
        let val = |v: u32| -> i32 {
            if v >= n { return new_val[(v - n) as usize] as i32; }
            match reval.binary_search_by_key(&v, |r| r.0) { Ok(i) => reval[i].1 as i32, Err(_) => sh.val[v as usize] as i32 }
        };
        let tags = |v: u32| -> u8 { if v >= n { new_tags[(v - n) as usize] } else { sh.tags[v as usize] } };
        // Current (weight, edge value) of every touched shared edge.
        let mut cur: std::collections::BTreeMap<u32, (i16, i16)> = Default::default();
        for &(l, w) in &ov.reweight { let l = sh.pos(l); cur.insert(l, (w, sh.ev[l as usize])); }
        for &(v, _) in &reval { if v < n { for &l in &sh.rin[sh.rin_off[v as usize] as usize..sh.rin_off[v as usize + 1] as usize] { cur.entry(l).or_insert((sh.edges[l as usize].w, sh.ev[l as usize])); } } }
        // Added edges, deduplicated as Builder::build does: max weight, then min value.
        let mut add = ov.added.clone();
        add.sort_unstable_by_key(|l| (l.0, l.1, l.2, std::cmp::Reverse(l.3), l.4));
        add.dedup_by_key(|l| (l.0, l.1, l.2));
        let mut extras: Vec<(u32, PEdge)> = vec![];
        for &(s, t, ty, w, ev) in &add {
            let hit = if s < n { sh.range(s).find(|&l| sh.edges[l].to == t && sh.ty[l] == ty) } else { None };
            match hit {
                Some(l) => {
                    let l = l as u32;
                    let c = *cur.get(&l).unwrap_or(&(sh.edges[l as usize].w, sh.ev[l as usize]));
                    // keep the larger weight; on a tie, the smaller value
                    let keep = if (w, std::cmp::Reverse(ev)) > (c.0, std::cmp::Reverse(c.1)) { (w, ev) } else { c };
                    cur.insert(l, keep);
                }
                None => extras.push((s, PEdge { to: t, w, key: gkey(ty, tags(t), val(t) + ev as i32) })),
            }
        }
        let mut ovr: Vec<(u32, u32, i16, u16)> = vec![]; // (source, edge, w, key)
        let mut u = 0u32;
        for (&l, &(w, ev)) in &cur {
            while sh.off[u as usize + 1] <= l { u += 1; }
            let e = sh.edges[l as usize];
            let key = gkey(sh.ty[l as usize], tags(e.to), val(e.to) + ev as i32);
            if w != e.w || key != e.key { ovr.push((u, l, w, key)); }
        }
        let mut d = Delta { base_n: n, reval, new_val, new_tags, ..Default::default() };
        let mut srcs: Vec<u32> = ovr.iter().map(|o| o.0).chain(extras.iter().map(|x| x.0)).collect();
        srcs.sort_unstable(); srcs.dedup();
        let (mut oi, mut xi) = (0, 0);
        for &s in &srcs {
            d.ostart.push(d.ovr.len() as u32); d.xstart.push(d.extra.len() as u32);
            let mut sum: i32 = if s < n { sh.range(s).map(|l| sh.edges[l].w.max(0) as i32).sum() } else { 0 };
            while oi < ovr.len() && ovr[oi].0 == s { let o = ovr[oi]; sum += o.2.max(0) as i32 - sh.edges[o.1 as usize].w.max(0) as i32; d.ovr.push(((o.1 - sh.off[s as usize]) as u16, o.2, o.3)); oi += 1; }
            while xi < extras.len() && extras[xi].0 == s { sum += extras[xi].1.w.max(0) as i32; d.extra.push(extras[xi].1); xi += 1; }
            d.den.push(sum.max(1000));
            let mut mk = 0u32;
            d.for_edges_at(sh, s, d.den.len() - 1, |_, _, w, k| if w > 0 { mk |= 1 << crate::shared::kclass(k); });
            d.pmask.push(mk);
        }
        d.ostart.push(d.ovr.len() as u32); d.xstart.push(d.extra.len() as u32);
        let total = n as usize + d.new_val.len();
        d.rank = vec![[0; 3]; total / 64 + 1];
        for (i, &s) in srcs.iter().enumerate() {
            let b = &mut d.rank[s as usize / 64];
            if b[0] == 0 && b[1] == 0 { b[2] = i as u32; }
            let bit = s % 64; b[(bit / 32) as usize] |= 1 << (bit % 32);
        }
        d
    }

    /// Bytes the kernels read per instance.
    pub fn bytes(&self) -> usize { self.den.len() * 16 + self.ovr.len() * 6 + self.extra.len() * 8 + self.rank.len() * 12 + 8 }
    /// Bytes of the `Form` view's extra tables (values of revalued and new nodes).
    pub fn view_bytes(&self) -> usize { self.reval.len() * 6 + self.new_val.len() * 3 }
    pub fn len(&self) -> usize { self.base_n as usize + self.new_val.len() }
    pub fn is_empty(&self) -> bool { self.len() == 0 }

    /// Index of `u` among the patched sources, if it is one.
    #[inline]
    pub fn find(&self, u: u32) -> Option<usize> {
        let b = self.rank[u as usize / 64];
        let mask = b[0] as u64 | (b[1] as u64) << 32;
        let bit = u % 64;
        if mask >> bit & 1 == 0 { return None; }
        Some(b[2] as usize + (mask & ((1u64 << bit) - 1)).count_ones() as usize)
    }

    /// Visit the instance's edges out of `u`: `f(den, to, w, key)`. Returns the edge count.
    #[inline(always)]
    pub fn for_edges(&self, sh: &Shared, u: u32, mut f: impl FnMut(i32, u32, i16, u16)) -> usize {
        match self.find(u) {
            None if u >= self.base_n => 0,
            None => { let es = &sh.edges[sh.range(u)]; let den = sh.den[u as usize]; for e in es { f(den, e.to, e.w, e.key); } es.len() }
            Some(i) => self.for_edges_at(sh, u, i, f),
        }
    }

    /// As `for_edges` for the `i`-th patched source `u` (its ovr/extra ranges must be built).
    pub fn for_edges_at(&self, sh: &Shared, u: u32, i: usize, mut f: impl FnMut(i32, u32, i16, u16)) -> usize {
        let den = self.den[i];
        let mut f = |to, w, k| f(den, to, w, k);
        let mut c = 0;
        let ov = &self.ovr[self.ostart[i] as usize..self.ostart.get(i + 1).map_or(self.ovr.len(), |&x| x as usize)];
        if u < self.base_n {
            let r = sh.range(u);
            let mut j = 0;
            let start = r.start;
            for l in r {
                let e = &sh.edges[l];
                if j < ov.len() && ov[j].0 as usize == l - start { f(e.to, ov[j].1, ov[j].2); j += 1; } else { f(e.to, e.w, e.key); }
                c += 1;
            }
        }
        for e in &self.extra[self.xstart[i] as usize..self.xstart.get(i + 1).map_or(self.extra.len(), |&x| x as usize)] { f(e.to, e.w, e.key); c += 1; }
        c
    }

    pub fn val(&self, sh: &Shared, v: u32) -> i32 {
        if v >= self.base_n { return self.new_val[(v - self.base_n) as usize] as i32; }
        match self.reval.binary_search_by_key(&v, |r| r.0) { Ok(i) => self.reval[i].1 as i32, Err(_) => sh.val[v as usize] as i32 }
    }
    pub fn tags(&self, sh: &Shared, v: u32) -> u8 { if v >= self.base_n { self.new_tags[(v - self.base_n) as usize] } else { sh.tags[v as usize] } }
}

/// `Form` view of shared graph + delta, for checking against the materialised graph. Edges come
/// back in an unspecified order (the kernels' results do not depend on edge order).
pub struct View<'a> { pub sh: &'a Shared, pub d: &'a Delta }
impl<'a> Form for View<'a> {
    fn len(&self) -> usize { self.d.len() }
    fn val(&self, n: u32) -> i32 { self.d.val(self.sh, n) }
    fn tags(&self, n: u32) -> u8 { self.d.tags(self.sh, n) }
    fn edges(&self, u: u32, out: &mut Vec<Edge>) {
        // The view reports keys, not raw (type, value): decode a representative triple.
        self.d.for_edges(self.sh, u, |_, to, w, key| if w > 0 {
            let (ty, _tg, v) = crate::shared::unkey(key);
            out.push(Edge { to, w: w as i32, ty, val: v - self.d.val(self.sh, to) });
        });
    }
}
