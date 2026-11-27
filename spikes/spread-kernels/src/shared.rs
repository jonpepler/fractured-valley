//! The graph all instances share, packed for the kernels.
//!
//! Every edge carries a precomputed *gain key*: the oracle's `gain(m, ty, tags[to], val[to] + ev)`
//! depends only on (type class, three tag bits, clamped value) — found by probing the oracle and
//! checked in `tests/gain_classes.rs`. A key fits in 16 bits (32 classes x 2001 values), so an
//! edge packs into 8 bytes: target, weight, key.

use mc::graph::Graph;

pub const TY_CLASS: [u8; 8] = [0, 0, 0, 0, 1, 2, 0, 3];
/// A representative link type for each type class.
pub const TY_REP: [u8; 4] = [0, 4, 5, 7];
pub const KEYS: usize = 32 * 2001;

#[inline]
pub fn tag_class(t: u8) -> u8 { (t & 3) | ((t >> 7) << 2) }
#[inline]
pub fn gkey(ty: u8, tags: u8, v: i32) -> u16 {
    ((TY_CLASS[ty as usize & 7] as u32 * 8 + tag_class(tags) as u32) * 2001 + (v.clamp(-1000, 1000) + 1000) as u32) as u16
}
/// Inverse of `gkey`: a (ty, tags, val) triple with that key.
#[inline]
pub fn unkey(k: u16) -> (u8, u8, i32) {
    let k = k as u32; let v = (k % 2001) as i32 - 1000; let c = k / 2001;
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
    /// Reverse adjacency: for each node, the indices of the edges into it.
    pub rin_off: Vec<u32>,
    pub rin: Vec<u32>,
}

impl Shared {
    pub fn new(g: &Graph) -> Shared {
        let n = g.len();
        let tags: Vec<u8> = g.tags.iter().map(|&t| t & !mc::graph::tag::ANCHORED).collect();
        let mut edges = Vec::with_capacity(g.link_count());
        let mut den = vec![0; n];
        for u in 0..n as u32 {
            let mut s = 0;
            for l in g.links(u) {
                let to = g.targets[l]; let w = g.slow[l];
                edges.push(PEdge { to, w, key: gkey(g.ltype[l], tags[to as usize], g.val[to as usize] as i32 + g.lval[l] as i32) });
                if w > 0 { s += w as i32; }
            }
            den[u as usize] = s.max(1000);
        }
        let mut rin_off = vec![0u32; n + 1];
        for e in &edges { rin_off[e.to as usize + 1] += 1; }
        for i in 0..n { rin_off[i + 1] += rin_off[i]; }
        let mut fill = rin_off.clone();
        let mut rin = vec![0u32; edges.len()];
        for (l, e) in edges.iter().enumerate() { let f = &mut fill[e.to as usize]; rin[*f as usize] = l as u32; *f += 1; }
        Shared { n, off: g.offsets.clone(), edges, ty: g.ltype.clone(), ev: g.lval.clone(), val: g.val.clone(), tags, den, rin_off, rin }
    }
    pub fn bytes(&self) -> usize { self.off.len() * 4 + self.edges.len() * 8 + self.ty.len() + self.ev.len() * 2 + self.val.len() * 2 + self.tags.len() + self.den.len() * 4 }
    /// Bytes the kernels read (without the reverse adjacency, which only building deltas needs).
    pub fn kernel_bytes(&self) -> usize { self.off.len() * 4 + self.edges.len() * 8 + self.den.len() * 4 }
    #[inline] pub fn range(&self, u: u32) -> std::ops::Range<usize> { self.off[u as usize] as usize..self.off[u as usize + 1] as usize }
}
