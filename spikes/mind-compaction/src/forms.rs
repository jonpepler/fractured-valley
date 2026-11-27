//! Compact forms of one mind. Each implements `Form`, so it runs the reference think unchanged
//! and differs only in what its links decode to and how they sit in memory.

use crate::gen::Overlay;
use crate::graph::{Anchor, Builder, Graph};
use crate::rng::hash;
use crate::spread::{Edge, Form};

const ANCHOR_B: usize = std::mem::size_of::<Anchor>();

// ---------------------------------------------------------------------------------------------
// Baseline 2: the thought graph's quantised packed form, verbatim in behaviour.

/// Weight in one byte (steps of 4), type in a byte, node valence in a byte (steps of 8), link
/// valence dropped (`spikes/thought-graph` `packed.rs`).
pub struct Packed { pub offsets: Vec<u32>, pub targets: Vec<u32>, pub w: Vec<u8>, pub ty: Vec<u8>, pub val: Vec<i8>, pub tags: Vec<u8>, pub anchors: Vec<Anchor> }
impl Packed {
    pub fn compile(g: &Graph) -> Packed {
        let mut p = Packed { offsets: vec![0], targets: vec![], w: vec![], ty: vec![], val: vec![], tags: g.tags.clone(), anchors: g.anchors.clone() };
        let mut edges = vec![];
        for u in 0..g.len() as u32 {
            edges.clear();
            g.edges(u, &mut edges);
            for e in &edges { p.targets.push(e.to); p.w.push((e.w / 4).clamp(0, 250) as u8); p.ty.push(e.ty); }
            p.offsets.push(p.targets.len() as u32);
        }
        p.val = g.val.iter().map(|&v| (v as i32 / 8).clamp(-127, 127) as i8).collect();
        p
    }
    pub fn bytes(&self) -> usize { self.offsets.len() * 4 + self.targets.len() * 4 + self.w.len() + self.ty.len() + self.val.len() + self.tags.len() + self.anchors.len() * ANCHOR_B }
}
impl Form for Packed {
    fn len(&self) -> usize { self.val.len() }
    #[inline] fn val(&self, n: u32) -> i32 { self.val[n as usize] as i32 * 8 }
    #[inline] fn tags(&self, n: u32) -> u8 { self.tags[n as usize] }
    #[inline]
    fn edges(&self, u: u32, out: &mut Vec<Edge>) {
        for l in self.offsets[u as usize] as usize..self.offsets[u as usize + 1] as usize {
            let w = self.w[l] as i32 * 4;
            if w > 0 { out.push(Edge { to: self.targets[l], w, ty: self.ty[l], val: 0 }); }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Weight precision. All keep link valence (one byte, steps of 8) and node valence (one byte).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec { U16, U8, U4Lin, U4Log, U3Log, U2Log }
impl Codec {
    /// Decoded weights for each code (code 0 is "no weight").
    pub fn levels(self) -> &'static [i32] {
        match self {
            Codec::U16 | Codec::U8 => &[],
            Codec::U4Lin => &[0, 67, 133, 200, 267, 333, 400, 467, 533, 600, 667, 733, 800, 867, 933, 1000],
            Codec::U4Log => &[0, 20, 26, 35, 46, 61, 81, 107, 141, 187, 247, 327, 432, 571, 756, 1000],
            Codec::U3Log => &[0, 20, 45, 100, 200, 350, 600, 1000],
            Codec::U2Log => &[0, 60, 250, 800],
        }
    }
    pub fn name(self) -> &'static str {
        match self { Codec::U16 => "u16", Codec::U8 => "u8 (steps of 4)", Codec::U4Lin => "4-bit linear", Codec::U4Log => "4-bit log", Codec::U3Log => "3-bit log", Codec::U2Log => "2-bit log" }
    }
    fn encode(self, w: i32) -> u16 {
        match self {
            Codec::U16 => w as u16,
            Codec::U8 => (w / 4).clamp(0, 250) as u16,
            _ => {
                let lv = self.levels();
                // Nearest level, but never 0 for a real link.
                (1..lv.len()).min_by_key(|&k| ((lv[k] - w).abs(), k)).unwrap() as u16
            }
        }
    }
}

/// CSR with the weight stored at a chosen precision. For the sub-byte codecs the weight and the
/// 3-bit type share one byte.
pub struct Quant { pub codec: Codec, pub offsets: Vec<u32>, pub targets: Vec<u32>, pub w16: Vec<u16>, pub w8: Vec<u8>, pub ty: Vec<u8>, pub lval: Vec<i8>, pub val: Vec<i8>, pub tags: Vec<u8>, pub anchors: Vec<Anchor>, lut: [i32; 256] }
impl Quant {
    pub fn compile(g: &Graph, codec: Codec) -> Quant {
        let mut lut = [0i32; 256];
        match codec {
            Codec::U16 => {}
            Codec::U8 => { for (k, x) in lut.iter_mut().enumerate() { *x = k as i32 * 4; } }
            _ => { for (k, &l) in codec.levels().iter().enumerate() { for t in 0..8 { lut[(k << 3) | t] = l; } } }
        }
        let mut q = Quant { codec, offsets: vec![0], targets: vec![], w16: vec![], w8: vec![], ty: vec![], lval: vec![], val: vec![], tags: g.tags.clone(), anchors: g.anchors.clone(), lut };
        let mut edges = vec![];
        for u in 0..g.len() as u32 {
            edges.clear();
            g.edges(u, &mut edges);
            for e in &edges {
                q.targets.push(e.to);
                q.lval.push((e.val / 8).clamp(-127, 127) as i8);
                let c = codec.encode(e.w);
                match codec {
                    Codec::U16 => { q.w16.push(c); q.ty.push(e.ty); }
                    Codec::U8 => { q.w8.push(c as u8); q.ty.push(e.ty); }
                    _ => q.w8.push(((c as u8) << 3) | e.ty),
                }
            }
            q.offsets.push(q.targets.len() as u32);
        }
        q.val = g.val.iter().map(|&v| (v as i32 / 8).clamp(-127, 127) as i8).collect();
        q
    }
    pub fn bytes(&self) -> usize {
        self.offsets.len() * 4 + self.targets.len() * 4 + self.w16.len() * 2 + self.w8.len() + self.ty.len() + self.lval.len() + self.val.len() + self.tags.len() + self.anchors.len() * ANCHOR_B
    }
}
impl Form for Quant {
    fn len(&self) -> usize { self.val.len() }
    #[inline] fn val(&self, n: u32) -> i32 { self.val[n as usize] as i32 * 8 }
    #[inline] fn tags(&self, n: u32) -> u8 { self.tags[n as usize] }
    #[inline]
    fn edges(&self, u: u32, out: &mut Vec<Edge>) {
        let r = self.offsets[u as usize] as usize..self.offsets[u as usize + 1] as usize;
        match self.codec {
            Codec::U16 => for l in r { let w = self.w16[l] as i32; if w > 0 { out.push(Edge { to: self.targets[l], w, ty: self.ty[l], val: self.lval[l] as i32 * 8 }); } },
            Codec::U8 => for l in r { let w = self.lut[self.w8[l] as usize]; if w > 0 { out.push(Edge { to: self.targets[l], w, ty: self.ty[l], val: self.lval[l] as i32 * 8 }); } },
            _ => for l in r { let b = self.w8[l]; let w = self.lut[b as usize]; if w > 0 { out.push(Edge { to: self.targets[l], w, ty: b & 7, val: self.lval[l] as i32 * 8 }); } },
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Compact: gap-coded targets (WebGraph / Ligra+ style byte codes) and one byte per link for
// weight and type, with link valence inline only where there is one.

/// Per link: a zigzag varint of the gap to the previous target (the first gap is from the
/// source), then a byte `w4log << 4 | type << 1 | has_valence`, then an i8 valence if flagged.
/// Node valence is one byte. Works best on a locality-ordered mind. `wide`: the weight gets its
/// own byte (steps of 4) and the flag byte keeps only type and valence flag.
pub struct Compact { pub offsets: Vec<u32>, pub stream: Vec<u8>, pub val: Vec<i8>, pub tags: Vec<u8>, pub anchors: Vec<Anchor>, pub wide: bool }
const LOG4: [i32; 16] = [0, 20, 26, 35, 46, 61, 81, 107, 141, 187, 247, 327, 432, 571, 756, 1000];
impl Compact {
    pub fn compile(g: &Graph) -> Compact { Self::compile_with(g, false) }
    pub fn compile_with(g: &Graph, wide: bool) -> Compact {
        let mut c = Compact { offsets: vec![0], stream: vec![], val: g.val.iter().map(|&v| (v as i32 / 8).clamp(-127, 127) as i8).collect(), tags: g.tags.clone(), anchors: g.anchors.clone(), wide };
        let mut edges = vec![];
        for u in 0..g.len() as u32 {
            edges.clear();
            g.edges(u, &mut edges);
            let mut prev = u as i64;
            for e in &edges {
                let gap = e.to as i64 - prev;
                prev = e.to as i64;
                let mut z = ((gap << 1) ^ (gap >> 63)) as u64;
                loop { let b = (z & 127) as u8; z >>= 7; if z == 0 { c.stream.push(b); break; } c.stream.push(b | 128); }
                let lv = (e.val / 8).clamp(-127, 127) as i8;
                if wide {
                    c.stream.push((e.ty << 1) | (lv != 0) as u8);
                    c.stream.push((e.w / 4).clamp(1, 250) as u8);
                } else {
                    let code = Codec::U4Log.encode(e.w) as u8;
                    c.stream.push((code << 4) | (e.ty << 1) | (lv != 0) as u8);
                }
                if lv != 0 { c.stream.push(lv as u8); }
            }
            c.offsets.push(c.stream.len() as u32);
        }
        c
    }
    pub fn bytes(&self) -> usize { self.offsets.len() * 4 + self.stream.len() + self.val.len() + self.tags.len() + self.anchors.len() * ANCHOR_B }
}
impl Form for Compact {
    fn len(&self) -> usize { self.val.len() }
    #[inline] fn val(&self, n: u32) -> i32 { self.val[n as usize] as i32 * 8 }
    #[inline] fn tags(&self, n: u32) -> u8 { self.tags[n as usize] }
    #[inline]
    fn edges(&self, u: u32, out: &mut Vec<Edge>) {
        let (mut i, end) = (self.offsets[u as usize] as usize, self.offsets[u as usize + 1] as usize);
        let s = &self.stream;
        let mut prev = u as i64;
        while i < end {
            let (mut z, mut shift) = (0u64, 0);
            loop { let b = s[i]; i += 1; z |= ((b & 127) as u64) << shift; shift += 7; if b < 128 { break; } }
            let gap = (z >> 1) as i64 ^ -((z & 1) as i64);
            prev += gap;
            let b = s[i]; i += 1;
            let w = if self.wide { let w = s[i] as i32 * 4; i += 1; w } else { LOG4[(b >> 4) as usize] };
            let val = if b & 1 != 0 { let v = s[i] as i8 as i32 * 8; i += 1; v } else { 0 };
            out.push(Edge { to: prev as u32, w, ty: (b >> 1) & 7, val });
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Array of structs: every field of a link in one record (versus the reference's struct of arrays).

#[derive(Clone, Copy)]
#[repr(C)]
pub struct LinkRec { pub to: u32, pub w: i16, pub val: i16, pub ty: u8 }
pub struct Aos { pub offsets: Vec<u32>, pub links: Vec<LinkRec>, pub val: Vec<i16>, pub tags: Vec<u8>, pub anchors: Vec<Anchor> }
impl Aos {
    pub fn compile(g: &Graph) -> Aos {
        let links = (0..g.link_count()).map(|l| LinkRec { to: g.targets[l], w: g.slow[l], val: g.lval[l], ty: g.ltype[l] }).collect();
        Aos { offsets: g.offsets.clone(), links, val: g.val.clone(), tags: g.tags.clone(), anchors: g.anchors.clone() }
    }
    pub fn bytes(&self) -> usize { self.offsets.len() * 4 + self.links.len() * std::mem::size_of::<LinkRec>() + self.val.len() * 2 + self.tags.len() + self.anchors.len() * ANCHOR_B }
}
impl Form for Aos {
    fn len(&self) -> usize { self.val.len() }
    #[inline] fn val(&self, n: u32) -> i32 { self.val[n as usize] as i32 }
    #[inline] fn tags(&self, n: u32) -> u8 { self.tags[n as usize] }
    #[inline]
    fn edges(&self, u: u32, out: &mut Vec<Edge>) {
        for r in &self.links[self.offsets[u as usize] as usize..self.offsets[u as usize + 1] as usize] {
            if r.w > 0 { out.push(Edge { to: r.to, w: r.w as i32, ty: r.ty, val: r.val as i32 }); }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Pruning.

/// Drop links weaker than `min_w`, and keep at most `top_k` strongest per node (0: no cap).
pub fn prune(g: &Graph, min_w: i16, top_k: usize) -> Graph {
    let mut b = Builder { val: g.val.clone(), tags: g.tags.clone(), anchors: g.anchors.clone(), links: vec![] };
    for u in 0..g.len() as u32 {
        let mut ls: Vec<usize> = g.links(u).filter(|&l| g.slow[l] >= min_w).collect();
        if top_k > 0 && ls.len() > top_k {
            ls.sort_by_key(|&l| (std::cmp::Reverse(g.slow[l]), l));
            ls.truncate(top_k);
        }
        for l in ls { b.links.push((u, g.targets[l], g.ltype[l], g.slow[l], g.lval[l])); }
    }
    b.build()
}

// ---------------------------------------------------------------------------------------------
// Locality orders. Each returns `perm`, where node `i` becomes `perm[i]`.

/// A seeded random order: a mind whose ids follow the order things were learnt, not structure.
pub fn shuffled(n: usize, seed: u64) -> Vec<u32> {
    let mut keys: Vec<(u64, u32)> = (0..n as u32).map(|i| (hash(seed, i as u64, 0x5a0f, 0), i)).collect();
    keys.sort_unstable();
    let mut perm = vec![0u32; n];
    for (new, &(_, old)) in keys.iter().enumerate() { perm[old as usize] = new as u32; }
    perm
}

fn undirected(g: &Graph) -> (Vec<u32>, Vec<u32>) {
    let n = g.len();
    let mut deg = vec![0u32; n + 1];
    for u in 0..n as u32 { for l in g.links(u) { deg[u as usize + 1] += 1; deg[g.targets[l] as usize + 1] += 1; } }
    for i in 0..n { deg[i + 1] += deg[i]; }
    let mut fill = deg.clone();
    let mut adj = vec![0u32; deg[n] as usize];
    for u in 0..n as u32 { for l in g.links(u) { let v = g.targets[l]; adj[fill[u as usize] as usize] = v; fill[u as usize] += 1; adj[fill[v as usize] as usize] = u; fill[v as usize] += 1; } }
    (deg, adj)
}

/// Breadth-first order from node 0 (then from each unvisited node in id order).
pub fn bfs(g: &Graph) -> Vec<u32> { traverse(g, false) }
/// Reverse Cuthill–McKee: BFS from a low-degree node, neighbours by ascending degree, reversed.
pub fn rcm(g: &Graph) -> Vec<u32> { traverse(g, true) }

fn traverse(g: &Graph, cm: bool) -> Vec<u32> {
    let n = g.len();
    let (off, adj) = undirected(g);
    let degree = |v: u32| off[v as usize + 1] - off[v as usize];
    let mut order: Vec<u32> = Vec::with_capacity(n);
    let mut seen = vec![false; n];
    let mut starts: Vec<u32> = (0..n as u32).collect();
    if cm { starts.sort_by_key(|&v| (degree(v), v)); }
    let mut nb = vec![];
    for &s in &starts {
        if seen[s as usize] { continue; }
        seen[s as usize] = true;
        let mut head = order.len();
        order.push(s);
        while head < order.len() {
            let u = order[head];
            head += 1;
            nb.clear();
            for &v in &adj[off[u as usize] as usize..off[u as usize + 1] as usize] { if !seen[v as usize] { seen[v as usize] = true; nb.push(v); } }
            if cm { nb.sort_by_key(|&v| (degree(v), v)); }
            order.extend_from_slice(&nb);
        }
    }
    if cm { order.reverse(); }
    let mut perm = vec![0u32; n];
    for (new, &old) in order.iter().enumerate() { perm[old as usize] = new as u32; }
    perm
}

/// Mean |source − target| over all links: how local a numbering is.
pub fn mean_gap(g: &Graph) -> u64 {
    let mut s = 0u64;
    for u in 0..g.len() as u32 { for l in g.links(u) { s += (g.targets[l] as i64 - u as i64).unsigned_abs(); } }
    s / g.link_count().max(1) as u64
}

// ---------------------------------------------------------------------------------------------
// Shared species base plus a per-mind delta overlay (copy-on-write at link granularity).

/// An individual read through its species base: links of the base with the overlay's
/// reweights applied, then the overlay's added links. Lossless against the materialised mind.
pub struct Cow<'a> {
    pub base: &'a Graph,
    pub ov: &'a Overlay,
    /// Sources of the added links (parallel to `ov.added`), for a binary search.
    add_src: Vec<u32>,
    reval_nodes: Vec<u32>,
    re_links: Vec<u32>,
    /// Optionally, this mind's node valence as a dense array (2 B per node) instead of a sparse
    /// lookup on every link read.
    pub dense_val: Option<Vec<i16>>,
}
impl<'a> Cow<'a> {
    pub fn new(base: &'a Graph, ov: &'a Overlay) -> Cow<'a> {
        Cow { base, ov, add_src: ov.added.iter().map(|l| l.0).collect(), reval_nodes: ov.revalue.iter().map(|r| r.0).collect(), re_links: ov.reweight.iter().map(|r| r.0).collect(), dense_val: None }
    }
    pub fn dense(mut self) -> Self {
        let mut v = self.base.val.clone();
        for &(n, x) in &self.ov.revalue { v[n as usize] = x; }
        v.extend(self.ov.new_nodes.iter().map(|n| n.0));
        self.dense_val = Some(v);
        self
    }
    /// Per-mind bytes: the overlay (the key arrays here are the overlay's own keys, laid out as
    /// struct of arrays; they are counted once, in `Overlay::bytes`), plus the dense valence.
    pub fn bytes(&self) -> usize { self.ov.bytes() + self.dense_val.as_ref().map_or(0, |v| v.len() * 2) }
}
impl Form for Cow<'_> {
    fn len(&self) -> usize { self.base.len() + self.ov.new_nodes.len() }
    #[inline]
    fn val(&self, n: u32) -> i32 {
        if let Some(v) = &self.dense_val { return v[n as usize] as i32; }
        if n as usize >= self.base.len() { return self.ov.new_nodes[n as usize - self.base.len()].0 as i32; }
        match self.reval_nodes.binary_search(&n) { Ok(i) => self.ov.revalue[i].1 as i32, Err(_) => self.base.val[n as usize] as i32 }
    }
    #[inline]
    fn tags(&self, n: u32) -> u8 {
        if n as usize >= self.base.len() { self.ov.new_nodes[n as usize - self.base.len()].1 } else { self.base.tags[n as usize] }
    }
    #[inline]
    fn edges(&self, u: u32, out: &mut Vec<Edge>) {
        let g = self.base;
        if (u as usize) < g.len() {
            let r = g.links(u);
            let mut k = self.re_links.partition_point(|&l| (l as usize) < r.start);
            for l in r {
                let mut w = g.slow[l] as i32;
                if k < self.re_links.len() && self.re_links[k] as usize == l { w = self.ov.reweight[k].1 as i32; k += 1; }
                if w > 0 { out.push(Edge { to: g.targets[l], w, ty: g.ltype[l], val: g.lval[l] as i32 }); }
            }
        }
        let mut i = self.add_src.partition_point(|&s| s < u);
        while i < self.add_src.len() && self.add_src[i] == u {
            let a = self.ov.added[i];
            if a.3 > 0 { out.push(Edge { to: a.1, w: a.3 as i32, ty: a.2, val: a.4 as i32 }); }
            i += 1;
        }
    }
}

/// Bytes per mind if copy-on-write were done at page granularity instead: every page of
/// `page_nodes` consecutive nodes that the overlay touches is copied whole (at the reference's
/// 9 bytes per link and 3 per node), plus a 4-byte page table entry per page.
pub fn page_cow_bytes(base: &Graph, ov: &Overlay, page_nodes: usize) -> usize {
    let pages = base.len().div_ceil(page_nodes);
    let mut touched = vec![false; pages];
    for &(l, _) in &ov.reweight { let s = base.offsets.partition_point(|&o| o <= l) - 1; touched[s / page_nodes] = true; }
    for a in &ov.added { if (a.0 as usize) < base.len() { touched[a.0 as usize / page_nodes] = true; } }
    for &(n, _) in &ov.revalue { touched[n as usize / page_nodes] = true; }
    let mut bytes = pages * 4;
    for (p, &t) in touched.iter().enumerate() {
        if !t { continue; }
        let (s, e) = (p * page_nodes, ((p + 1) * page_nodes).min(base.len()));
        let links = (base.offsets[e] - base.offsets[s]) as usize;
        bytes += links * 9 + (e - s) * 3 + 4 * (e - s);
    }
    // New nodes and links from them live outside the base pages, as in the link overlay.
    bytes + ov.new_nodes.len() * 3 + ov.added.iter().filter(|a| a.0 as usize >= base.len()).count() * 13
}

// ---------------------------------------------------------------------------------------------
// Baseline 3: the hyperdimensional form (thought graph `hd.rs`, untyped), trimmed.

pub struct Hd { pub bits: usize, pub words: usize, pub n: usize, pub bundles: Vec<u64>, pub scale: Vec<u16>, pub degree: Vec<u8>, pub val: Vec<i16>, pub tags: Vec<u8>, pub codebook: Vec<u64>, pub thresh: i32, pub anchors: usize }
impl Hd {
    pub fn compile(g: &Graph, bits: usize, seed: u64) -> Hd {
        let words = bits / 64;
        let n = g.len();
        let codebook: Vec<u64> = (0..n * words).map(|i| hash(seed, (i / words) as u64, (i % words) as u64, 0x17e4_c0de)).collect();
        let mut bundles = vec![0u64; n * words];
        let (mut scale, mut degree) = (vec![0u16; n], vec![0u8; n]);
        let mut counts = vec![0i32; bits];
        let mut edges = vec![];
        let mut members: Vec<(u32, i32)> = vec![];
        for u in 0..n {
            edges.clear();
            g.edges(u as u32, &mut edges);
            members.clear();
            for e in &edges {
                if e.to as usize == u { continue; }
                match members.iter_mut().find(|m| m.0 == e.to) { Some(m) => m.1 = (m.1 + e.w).min(1000), None => members.push((e.to, e.w)) }
            }
            if members.is_empty() { continue; }
            degree[u] = members.len().min(255) as u8;
            counts.iter_mut().for_each(|c| *c = 0);
            for &(v, w) in &members {
                let copies = 1 + w * 3 / 1000;
                for wi in 0..words { let x = codebook[v as usize * words + wi]; for b in 0..64 { counts[wi * 64 + b] += if (x >> b) & 1 == 1 { copies } else { -copies }; } }
            }
            for wi in 0..words {
                let tie = hash(seed, u as u64, wi as u64, 0x71e_b17);
                let mut x = 0u64;
                for b in 0..64 { let c = counts[wi * 64 + b]; if c > 0 || (c == 0 && (tie >> b) & 1 == 1) { x |= 1 << b; } }
                bundles[u * words + wi] = x;
            }
            let (mut sw, mut se) = (0i64, 0i64);
            for &(v, w) in &members {
                let e = excess(&bundles[u * words..(u + 1) * words], &codebook[v as usize * words..(v as usize + 1) * words], bits);
                if e > 0 { sw += w as i64; se += e as i64; }
            }
            scale[u] = if se > 0 { (sw * 256 / se).min(u16::MAX as i64) as u16 } else { 0 };
        }
        Hd { bits, words, n, bundles, scale, degree, val: g.val.clone(), tags: g.tags.clone(), codebook, thresh: (bits as u32).isqrt() as i32 * 9 / 4, anchors: g.anchors.len() }
    }
    pub fn bytes(&self) -> usize { self.bundles.len() * 8 + self.scale.len() * 2 + self.degree.len() + self.val.len() * 2 + self.tags.len() + self.anchors * ANCHOR_B }
}
#[inline]
pub fn excess(a: &[u64], b: &[u64], bits: usize) -> i32 {
    let ham: u32 = a.iter().zip(b).map(|(x, y)| (x ^ y).count_ones()).sum();
    (bits as i32) / 2 - ham as i32
}
impl Form for Hd {
    fn len(&self) -> usize { self.n }
    fn val(&self, n: u32) -> i32 { self.val[n as usize] as i32 }
    fn tags(&self, n: u32) -> u8 { self.tags[n as usize] }
    fn edges(&self, u: u32, out: &mut Vec<Edge>) {
        if self.degree[u as usize] == 0 { return; }
        let bu = &self.bundles[u as usize * self.words..(u as usize + 1) * self.words];
        let tu = self.tags[u as usize];
        let sc = self.scale[u as usize] as i64;
        for v in 0..self.n as u32 {
            if v == u { continue; }
            let e = excess(bu, &self.codebook[v as usize * self.words..(v as usize + 1) * self.words], self.bits);
            if e < self.thresh { continue; }
            let w = ((e as i64 * sc / 256) as i32).min(1000);
            if w > 0 { out.push(Edge { to: v, w, ty: crate::gen::infer_type(tu, self.tags[v as usize]), val: 0 }); }
        }
    }
}

