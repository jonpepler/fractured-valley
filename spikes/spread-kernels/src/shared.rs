//! The graph all instances share, packed for the kernels.
//!
//! Every edge carries a precomputed *gain key*: the oracle's `gain(m, ty, tags[to], val[to] + ev)`
//! depends only on (type class, three tag bits, clamped value) — found by probing the oracle and
//! checked in `tests/adapter.rs`. A key fits in 16 bits (class << 11 | value + 1000), so an
//! edge packs into 8 bytes: target, weight, key.

use mc::graph::Graph;

pub const TY_CLASS: [u8; 8] = [0, 0, 0, 0, 1, 2, 0, 3];
/// A representative link type for each type class.
pub const TY_REP: [u8; 4] = [0, 4, 5, 7];
pub const KEYS: usize = 32 << 11;

#[inline]
pub fn tag_class(t: u8) -> u8 { (t & 3) | ((t >> 7) << 2) }
#[inline]
pub fn gkey(ty: u8, tags: u8, v: i32) -> u16 {
    ((TY_CLASS[ty as usize & 7] as u32 * 8 + tag_class(tags) as u32) << 11 | (v.clamp(-1000, 1000) + 1000) as u32) as u16
}
/// Inverse of `gkey`: a (ty, tags, val) triple with that key.
/// Gain class of a key (0..32) and its clamped value.
#[inline] pub fn kclass(k: u16) -> u32 { k as u32 >> 11 }
#[inline] pub fn kval(k: u16) -> i32 { (k as u32 & 2047) as i32 - 1000 }
#[inline]
pub fn unkey(k: u16) -> (u8, u8, i32) {
    let k = k as u32; let v = (k & 2047) as i32 - 1000; let c = k >> 11;
    let tc = (c & 7) as u8; (TY_REP[(c >> 3) as usize], (tc & 3) | ((tc >> 2) << 7), v)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct PEdge { pub to: u32, pub w: i16, pub key: u16 }

pub struct Shared {
    pub n: usize,
    pub off: Vec<u32>,
    pub edges: Vec<PEdge>,
    /// Raw link type and edge value, for re-keying when an instance revalues a target.
    pub ty: Vec<u8>,
    pub ev: Vec<i16>,
    pub val: Vec<i16>,
    pub tags: Vec<u8>,
    /// max(sum of positive weights, 1000) per node.
    pub den: Vec<i32>,
    /// Per node: bit c set if an edge with positive weight has a key of gain class c.
    pub cmask: Vec<u32>,
    /// Graph link index -> position in `edges`.
    pub pos: Vec<u32>,
    /// Reverse adjacency: for each node, the positions of the edges into it.
    pub rin_off: Vec<u32>,
    pub rin: Vec<u32>,
}

impl Shared {
    /// Edges of each node are stored by weight, heaviest first (the kernels' results do not
    /// depend on edge order; the pruned kernel uses it to stop early).
    pub fn new(g: &Graph) -> Shared {
        let n = g.len();
        let tags: Vec<u8> = g.tags.iter().map(|&t| t & !mc::graph::tag::ANCHORED).collect();
        let m = g.link_count();
        let (mut edges, mut ty, mut ev) = (Vec::with_capacity(m), Vec::with_capacity(m), Vec::with_capacity(m));
        let mut pos = vec![0u32; m];
        let mut den = vec![0; n];
        let mut cmask = vec![0u32; n];
        for u in 0..n as u32 {
            let mut ls: Vec<usize> = g.links(u).collect();
            ls.sort_by_key(|&l| (std::cmp::Reverse(g.slow[l]), l));
            let mut s = 0;
            for l in ls {
                let to = g.targets[l]; let w = g.slow[l];
                pos[l] = edges.len() as u32;
                let key = gkey(g.ltype[l], tags[to as usize], g.val[to as usize] as i32 + g.lval[l] as i32);
                edges.push(PEdge { to, w, key });
                ty.push(g.ltype[l]); ev.push(g.lval[l]);
                if w > 0 { s += w as i32; cmask[u as usize] |= 1 << (key >> 11); }
            }
            den[u as usize] = s.max(1000);
        }
        let mut rin_off = vec![0u32; n + 1];
        for e in &edges { rin_off[e.to as usize + 1] += 1; }
        for i in 0..n { rin_off[i + 1] += rin_off[i]; }
        let mut fill = rin_off.clone();
        let mut rin = vec![0u32; edges.len()];
        for (l, e) in edges.iter().enumerate() { let f = &mut fill[e.to as usize]; rin[*f as usize] = l as u32; *f += 1; }
        Shared { n, off: g.offsets.clone(), edges, ty, ev, val: g.val.clone(), tags, den, cmask, pos, rin_off, rin }
    }
    /// Position in `edges` of the graph's link `l`.
    #[inline] pub fn pos(&self, l: u32) -> u32 { self.pos[l as usize] }
    pub fn bytes(&self) -> usize { self.off.len() * 4 + self.edges.len() * 8 + self.ty.len() + self.ev.len() * 2 + self.val.len() * 2 + self.tags.len() + self.den.len() * 4 + self.cmask.len() * 4 + self.pos.len() * 4 + self.rin_off.len() * 4 + self.rin.len() * 4 }
    /// Bytes the kernels read (without the reverse adjacency, which only building deltas needs).
    pub fn kernel_bytes(&self) -> usize { self.off.len() * 4 + self.edges.len() * 8 + self.den.len() * 4 + self.cmask.len() * 4 }
    #[inline] pub fn range(&self, u: u32) -> std::ops::Range<usize> { self.off[u as usize] as usize..self.off[u as usize + 1] as usize }
}
