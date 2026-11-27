//! The compiled form of a mind: hyperdimensional bit vectors (Kanerva 2009).
//!
//! - Every node has a random **item vector** of `bits` bits, from `hash(seed, node, word)`. It's
//!   a codebook of labels, not knowledge, so it can be regenerated or shared between minds.
//! - Each node's out-links compile into one **bundle**: a majority vote over its targets' item
//!   vectors, each repeated 1–4 times by weight. Ties are broken by a seeded hash bit.
//! - **Thinking** reads a node's links back by **cleanup**: popcount similarity of its bundle
//!   against every item vector. Targets well above chance are its links; how far above chance
//!   gives the weight back through a per-node scale.
//! - Kept per node beside the bundle: valence, tags, a weight scale and a link count. Link
//!   valence is lost.
//! - **Link types** either are inferred from the two ends' tags (`typed: false`), or are kept by
//!   **binding**: each target enters the bundle as `item(v) XOR role(type)`, and cleanup probes
//!   once per type the node uses (`typed: true`, a one-byte type mask per node). Binding keeps
//!   the types but multiplies cleanup cost by the number of types probed.
//!
//! Lossy by design: weak links among many strong ones fall under the noise and are forgotten.

use crate::graph::{lt, tag, Anchor, Builder, Graph};
use crate::rng::hash;
use crate::spread::{Edge, Form};

const ITEM_SALT: u64 = 0x17e4_c0de;
const TIE_SALT: u64 = 0x71e_b17;
const ROLE_SALT: u64 = 0x2017_e5;

#[derive(Clone, Debug)]
pub struct Hd {
    pub bits: usize,
    pub words: usize,
    pub seed: u64,
    pub n: usize,
    pub bundles: Vec<u64>,
    /// Weight per unit of similarity above chance, ×256.
    pub scale: Vec<u16>,
    /// Number of links compiled in (0 means no cleanup is needed).
    pub degree: Vec<u8>,
    pub val: Vec<i16>,
    pub tags: Vec<u8>,
    pub anchors: Vec<(u32, Anchor)>,
    /// The item vectors (the codebook). Regenerable from the seed.
    pub codebook: Vec<u64>,
    /// Similarity above chance needed to count as a link.
    pub thresh: i32,
    pub typed: bool,
    /// Link types each node's bundle holds (typed mode).
    pub typemask: Vec<u8>,
    /// One role vector per link type (typed mode).
    pub roles: Vec<u64>,
}

/// A link type guessed from what the two ends are.
pub fn infer_type(tu: u8, tv: u8) -> u8 {
    if tu & tag::BODY != 0 { lt::WANTS }
    else if tu & tag::PLACE != 0 && tv & tag::PLACE != 0 { lt::NEAR }
    else if tu & tag::PLACE != 0 { if tv & tag::ANCHORED != 0 { lt::HOLDS } else { lt::FEELS } }
    else if tv & tag::PLACE != 0 { lt::AT }
    else { lt::KIND }
}

pub fn item_word(seed: u64, n: u32, w: usize) -> u64 { hash(seed, n as u64, w as u64, ITEM_SALT) }
pub fn role_word(seed: u64, t: u8, w: usize) -> u64 { hash(seed, t as u64, w as u64, ROLE_SALT) }

impl Hd {
    /// Compile any form (normally the live graph: slow + fast) into bit vectors.
    pub fn compile<F: Form>(f: &F, g: &Graph, bits: usize, seed: u64) -> Hd { Self::compile_with(f, g, bits, seed, false) }

    pub fn compile_with<F: Form>(f: &F, g: &Graph, bits: usize, seed: u64, typed: bool) -> Hd {
        assert!(bits % 64 == 0);
        let words = bits / 64;
        let n = f.len();
        let mut codebook = vec![0u64; n * words];
        for v in 0..n { for w in 0..words { codebook[v * words + w] = item_word(seed, v as u32, w); } }
        let mut bundles = vec![0u64; n * words];
        let mut scale = vec![0u16; n];
        let mut degree = vec![0u8; n];
        let mut counts = vec![0i32; bits];
        let mut edges = vec![];
        let mut members: Vec<(u32, u8, i32)> = vec![];
        let roles: Vec<u64> = if typed { (0..lt::COUNT).flat_map(|t| (0..words).map(move |w| role_word(seed, t as u8, w))).collect() } else { vec![] };
        let mut typemask = vec![0u8; if typed { n } else { 0 }];
        let mut bound = vec![0u64; words];
        for u in 0..n {
            edges.clear();
            f.edges(u as u32, &mut edges);
            // Untyped: merge parallel links (e.g. near + like between two places), weights add, capped.
            members.clear();
            for e in &edges {
                if e.to as usize == u { continue; }
                let ty = if typed { e.ty } else { 0 };
                match members.iter_mut().find(|m| m.0 == e.to && m.1 == ty) { Some(m) => m.2 = (m.2 + e.w).min(1000), None => members.push((e.to, ty, e.w)) }
            }
            if members.is_empty() { continue; }
            degree[u] = members.len().min(255) as u8;
            counts.iter_mut().for_each(|c| *c = 0);
            for &(v, ty, w) in &members {
                let copies = 1 + w * 3 / 1000;
                Self::bind(&codebook, &roles, typed, words, v, ty, &mut bound);
                if typed { typemask[u] |= 1 << ty; }
                for (wi, &x) in bound.iter().enumerate() {
                    let c = &mut counts[wi * 64..wi * 64 + 64];
                    for b in 0..64 { c[b] += if (x >> b) & 1 == 1 { copies } else { -copies }; }
                }
            }
            let out = &mut bundles[u * words..(u + 1) * words];
            for wi in 0..words {
                let tie = hash(seed, u as u64, wi as u64, TIE_SALT);
                let mut x = 0u64;
                for b in 0..64 {
                    let c = counts[wi * 64 + b];
                    if c > 0 || (c == 0 && (tie >> b) & 1 == 1) { x |= 1 << b; }
                }
                out[wi] = x;
            }
            // Calibrate: weight per unit of similarity above chance.
            let (mut sw, mut se) = (0i64, 0i64);
            for &(v, ty, w) in &members {
                Self::bind(&codebook, &roles, typed, words, v, ty, &mut bound);
                let e = Self::excess(out, &bound, bits);
                if e > 0 { sw += w as i64; se += e as i64; }
            }
            scale[u] = if se > 0 { (sw * 256 / se).min(u16::MAX as i64) as u16 } else { 0 };
        }
        let thresh = (bits as u32).isqrt() as i32 * 9 / 4;
        Hd { bits, words, seed, n, bundles, scale, degree, val: (0..n).map(|i| f.val(i as u32) as i16).collect(),
            tags: (0..n).map(|i| f.tags(i as u32)).collect(), anchors: g.anchors.clone(), codebook, thresh, typed, typemask, roles }
    }

    /// `item(v)`, or `item(v) XOR role(ty)` when typed.
    fn bind(codebook: &[u64], roles: &[u64], typed: bool, words: usize, v: u32, ty: u8, out: &mut [u64]) {
        let item = &codebook[v as usize * words..(v as usize + 1) * words];
        if typed { let r = &roles[ty as usize * words..(ty as usize + 1) * words]; for i in 0..words { out[i] = item[i] ^ r[i]; } }
        else { out.copy_from_slice(item); }
    }

    /// Similarity above chance, in bits: (agreeing bits) − bits/2.
    #[inline]
    pub fn excess(a: &[u64], b: &[u64], bits: usize) -> i32 {
        let ham: u32 = a.iter().zip(b).map(|(x, y)| (x ^ y).count_ones()).sum();
        (bits as i32) / 2 - ham as i32
    }

    pub fn bundle(&self, u: u32) -> &[u64] { &self.bundles[u as usize * self.words..(u as usize + 1) * self.words] }
    pub fn item(&self, v: u32) -> &[u64] { &self.codebook[v as usize * self.words..(v as usize + 1) * self.words] }

    /// Bytes per mind: bundles plus per-node metadata. The codebook is shared and counted apart.
    pub fn bytes(&self) -> usize {
        self.bundles.len() * 8 + self.scale.len() * 2 + self.degree.len() + self.val.len() * 2 + self.tags.len() + self.typemask.len()
            + self.anchors.len() * std::mem::size_of::<(u32, Anchor)>()
    }
    pub fn codebook_bytes(&self) -> usize { self.codebook.len() * 8 + self.roles.len() * 8 }

    /// Expand back into an approximate packed graph (for the viewer and for inspection).
    /// `names` is the debug label table; the compiled form has no names of its own.
    pub fn decompile(&self, names: &[String]) -> Graph {
        let mut b = Builder::default();
        for v in 0..self.n {
            let anchor = self.anchors.binary_search_by_key(&(v as u32), |a| a.0).ok().map(|i| self.anchors[i].1);
            b.node(names.get(v).map(|s| s.as_str()).unwrap_or("?"), self.val[v] as i32, self.tags[v] & !tag::ANCHORED, anchor);
        }
        let mut edges = vec![];
        for u in 0..self.n {
            edges.clear();
            self.edges(u as u32, &mut edges);
            for e in &edges { b.link(u as u32, e.to, e.ty, e.w); }
        }
        b.build()
    }
}

impl Form for Hd {
    fn len(&self) -> usize { self.n }
    fn val(&self, n: u32) -> i32 { self.val[n as usize] as i32 }
    fn tags(&self, n: u32) -> u8 { self.tags[n as usize] }
    fn edges(&self, u: u32, out: &mut Vec<Edge>) {
        if self.degree[u as usize] == 0 { return; }
        let bu = self.bundle(u);
        let tu = self.tags[u as usize];
        let sc = self.scale[u as usize] as i64;
        if !self.typed {
            for v in 0..self.n as u32 {
                if v == u { continue; }
                let e = Self::excess(bu, self.item(v), self.bits);
                if e < self.thresh { continue; }
                let w = ((e as i64 * sc / 256) as i32).min(1000);
                if w <= 0 { continue; }
                out.push(Edge { to: v, w, ty: infer_type(tu, self.tags[v as usize]), val: 0, link: u32::MAX });
            }
            return;
        }
        // Typed: unbind each role the node uses, then clean up against the codebook.
        let mut probe = vec![0u64; self.words];
        for t in 0..lt::COUNT as u8 {
            if self.typemask[u as usize] & (1 << t) == 0 { continue; }
            let r = &self.roles[t as usize * self.words..(t as usize + 1) * self.words];
            for i in 0..self.words { probe[i] = bu[i] ^ r[i]; }
            for v in 0..self.n as u32 {
                if v == u { continue; }
                let e = Self::excess(&probe, self.item(v), self.bits);
                if e < self.thresh { continue; }
                let w = ((e as i64 * sc / 256) as i32).min(1000);
                if w <= 0 { continue; }
                out.push(Edge { to: v, w, ty: t, val: 0, link: u32::MAX });
            }
        }
    }
}
