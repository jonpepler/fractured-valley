//! Two forms of one mind (decision 8.1), behind one interface ([`Store`]):
//!
//! - **[`Runtime`]**, the form a mind runs on (the compaction lab's recommendation, 8.6):
//!   - a **species [`Base`]**, shared by every mind of the species (`Arc`): gap-coded targets
//!     (zigzag varints, WebGraph/Ligra+ style), one byte for type and a valence flag, one byte of
//!     weight (steps of 4), and link valence only where there is one (one byte, steps of 8);
//!   - a **per-mind [`Overlay`]**: dense node valence, the mind's own anchors, sparse reweights and
//!     link valences of base links, and the nodes and links the mind added itself (new places,
//!     told facts);
//!   - **sparse fast weights** (only links with a trace are stored).
//! - **[`Expanded`]**, the inspectable form for the viewer, tests and debugging: plain CSR arrays
//!   with every value spelled out. It holds exactly the same values, so the two think the same.
//!
//! [`Runtime::expand`] and [`Expanded::compile`] convert between them losslessly; tests check
//! that both forms give identical hashes tick by tick. Learning (fast weights, consolidation,
//! adding nodes and links) is done through the same interface on either form.
//!
//! A link is addressed by its [`LinkKey`]: (source, position among the source's links), with the
//! base's links first in base order, then the mind's added links in the order they were added.

use crate::graph::{link_key, link_pos, link_src, node_key, tag, Anchor, Builder, LinkKey, LINK_BITS};
use crate::think::{Edge, Form};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// One link, as either form reports it (every link, including zero-weight slots).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Link { pub key: LinkKey, pub to: u32, pub ty: u8, pub slow: i16, pub lval: i16, pub fast: i16 }
impl Link { pub fn w(&self) -> i32 { (self.slow as i32 + self.fast as i32).min(1000) } }

/// Storage of a mind, in bytes, by part.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bytes {
    /// The species base (shared by every mind of a species; 0 for a form that shares nothing).
    pub shared: usize,
    /// The mind's own long-term part: overlay (or the whole expanded graph).
    pub own: usize,
    /// Fast weights (sparse in the runtime form, dense in the expanded form).
    pub fast: usize,
}

/// A form of a mind that can be thought with, read, and learnt into.
pub trait Store: Form + Clone + Send + Sync {
    fn name(&self, n: u32) -> &str;
    fn set_val(&mut self, n: u32, v: i16);
    fn anchor(&self, n: u32) -> Option<Anchor>;
    fn set_anchor(&mut self, n: u32, a: Anchor);
    /// Every link of `u`, in key order, appended to `out`.
    fn links(&self, u: u32, out: &mut Vec<Link>);
    fn link(&self, key: LinkKey) -> Link;
    fn set_slow(&mut self, key: LinkKey, w: i16);
    fn set_lval(&mut self, key: LinkKey, v: i16);
    fn set_fast(&mut self, key: LinkKey, f: i16);
    /// Non-zero fast weights in key order.
    fn fast_entries(&self) -> Vec<(LinkKey, i16)>;
    /// Apply `f` to every non-zero fast weight.
    fn map_fast(&mut self, f: &mut dyn FnMut(i16) -> i16);
    fn fast_total(&self) -> i64;
    /// A new node, with a stable key from the mind's `origin` (it is the mind's, not the species').
    fn add_node(&mut self, name: &str, val: i32, tags: u8, anchor: Option<Anchor>, origin: u64) -> u32;
    fn add_link(&mut self, u: u32, v: u32, ty: u8, w: i32, lval: i32) -> LinkKey;
    fn bytes(&self) -> Bytes;
    fn link_count(&self) -> usize;
    /// How many of `u`'s links came from the species base (the rest are the mind's own).
    fn base_degree(&self, u: u32) -> usize;
    /// Remove one of the mind's own links (never a base link). The links after it on the same
    /// node move down one position, so their keys (and fast weights) move with them.
    fn remove_link(&mut self, key: LinkKey);
    /// Nodes that have links of the mind's own, in id order.
    fn own_sources(&self) -> Vec<u32> { (0..self.len() as u32).filter(|&u| self.links_vec(u).len() > self.base_degree(u)).collect() }

    fn slow(&self, key: LinkKey) -> i16 { self.link(key).slow }
    fn fast(&self, key: LinkKey) -> i16 { self.link(key).fast }
    fn links_vec(&self, u: u32) -> Vec<Link> { let mut v = vec![]; self.links(u, &mut v); v }
    /// The first link u → v of a type.
    fn find(&self, u: u32, v: u32, ty: u8) -> Option<LinkKey> { self.links_vec(u).into_iter().find(|l| l.to == v && l.ty == ty).map(|l| l.key) }
    fn id(&self, name: &str) -> u32 { (0..self.len() as u32).find(|&n| self.name(n) == name).unwrap_or_else(|| panic!("no node {name}")) }
    /// Every slow weight, in key order (for checking that nothing changed).
    fn slow_all(&self) -> Vec<i16> { (0..self.len() as u32).flat_map(|u| self.links_vec(u).into_iter().map(|l| l.slow)).collect() }
    /// The mind's long-term contents as a form-independent hash: every node (valence, tags, key,
    /// anchor) and every link (target, type, slow, valence, fast), in id and key order.
    fn hash_into<H: Hasher>(&self, h: &mut H) where Self: Sized {
        let n = self.len();
        n.hash(h);
        let mut ls = vec![];
        for u in 0..n as u32 {
            self.val(u).hash(h); self.tags(u).hash(h); self.key(u).hash(h); self.anchor(u).hash(h);
            ls.clear(); self.links(u, &mut ls);
            ls.len().hash(h);
            for l in &ls { (l.to, l.ty, l.slow, l.lval, l.fast).hash(h); }
        }
    }
}

// ------------------------------------------------------------------------------------- sparse

/// A sorted sparse map from link key to a value (no zero values for fast weights; values that
/// differ from the base for overrides).
#[derive(Clone, Debug, Default, Hash, PartialEq, Eq)]
pub struct Sparse { pub keys: Vec<LinkKey>, pub vals: Vec<i16> }
impl Sparse {
    #[inline] pub fn get(&self, k: LinkKey) -> Option<i16> { self.keys.binary_search(&k).ok().map(|i| self.vals[i]) }
    pub fn set(&mut self, k: LinkKey, v: Option<i16>) {
        match (self.keys.binary_search(&k), v) {
            (Ok(i), Some(v)) => self.vals[i] = v,
            (Ok(i), None) => { self.keys.remove(i); self.vals.remove(i); }
            (Err(i), Some(v)) => { self.keys.insert(i, k); self.vals.insert(i, v); }
            (Err(_), None) => {}
        }
    }
    /// First index at or after the first link of `u`.
    #[inline] pub fn start(&self, u: u32) -> usize { self.keys.partition_point(|&k| k < (u as u64) << LINK_BITS) }
    pub fn len(&self) -> usize { self.keys.len() }
    pub fn is_empty(&self) -> bool { self.keys.is_empty() }
    /// As held: a u64 key and an i16 value, 10 bytes an entry (capacity slack not counted;
    /// a packed (u32, u16, i16) record would be 8).
    pub fn bytes(&self) -> usize { self.keys.len() * 10 }
}

// ------------------------------------------------------------------------------------- base

/// A species' shared founding graph, compiled: gap-coded, u8 weights, sparse link valence.
#[derive(Debug)]
pub struct Base {
    /// Byte offset of each node's links in `stream` (n + 1).
    pub offsets: Vec<u32>,
    pub stream: Vec<u8>,
    /// Founding node valence (each mind copies it into its dense overlay valence).
    pub val: Vec<i16>,
    pub tags: Vec<u8>,
    pub keys: Vec<u32>,
    /// Founding anchors (each mind copies them: anchors move with what the mind sees).
    pub anchors: Vec<(u32, Anchor)>,
    /// Debug labels only.
    pub names: Vec<String>,
    pub links: usize,
}

/// One decoded base link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BaseLink { pub to: u32, pub ty: u8, pub w: i16, pub lval: i16 }

/// Weights are stored in steps of 4, rounded to nearest (0..1000 → 0..250).
#[inline] pub fn quant_w(w: i16) -> i16 { ((w as i32 + 2) / 4 * 4).clamp(0, 1000) as i16 }
/// Link valence is stored in steps of 8, rounded toward zero.
#[inline] pub fn quant_v(v: i16) -> i16 { (v / 8) * 8 }

impl Base {
    /// Compile a founding graph. Weights are quantised (`quant_w`, `quant_v`).
    pub fn compile(mut b: Builder) -> Base { b.canonical(); Self::compile_ordered(b) }
    /// Compile links in the order given (grouped by source), without sorting or merging them, so
    /// every link keeps its position (and so its `LinkKey`).
    pub fn compile_ordered(mut b: Builder) -> Base {
        b.anchors.sort_by_key(|a| a.0);
        assert!(b.links.windows(2).all(|w| w[0].0 <= w[1].0), "links must be grouped by source");
        let n = b.len();
        let mut offsets = vec![0u32; n + 1];
        let mut stream = vec![];
        let mut li = 0;
        for u in 0..n as u32 {
            let mut prev = u as i64;
            let first = li;
            while li < b.links.len() && b.links[li].0 == u {
                // A link key holds the position in LINK_BITS bits.
                assert!(li - first < 1 << LINK_BITS, "node {u} has more than {} links", 1u32 << LINK_BITS);
                let (_, t, ty, w, v) = b.links[li];
                let gap = t as i64 - prev;
                prev = t as i64;
                let mut z = ((gap << 1) ^ (gap >> 63)) as u64;
                loop { let x = (z & 127) as u8; z >>= 7; if z == 0 { stream.push(x); break; } stream.push(x | 128); }
                let lv = (quant_v(v) / 8) as i8;
                stream.push((ty << 1) | (lv != 0) as u8);
                stream.push((quant_w(w) / 4) as u8);
                if lv != 0 { stream.push(lv as u8); }
                li += 1;
            }
            offsets[u as usize + 1] = stream.len() as u32;
        }
        Base { offsets, stream, val: b.val, tags: b.tags, keys: b.keys, anchors: b.anchors, names: b.names, links: b.links.len() }
    }
    pub fn len(&self) -> usize { self.val.len() }
    pub fn is_empty(&self) -> bool { self.val.is_empty() }
    /// Decode node `u`'s links, calling `f(position, link)`.
    #[inline]
    pub fn decode(&self, u: u32, mut f: impl FnMut(usize, BaseLink)) {
        let (mut i, end) = (self.offsets[u as usize] as usize, self.offsets[u as usize + 1] as usize);
        let s = &self.stream;
        let mut prev = u as i64;
        let mut k = 0;
        while i < end {
            let (mut z, mut shift) = (0u64, 0);
            loop { let b = s[i]; i += 1; z |= ((b & 127) as u64) << shift; shift += 7; if b < 128 { break; } }
            prev += (z >> 1) as i64 ^ -((z & 1) as i64);
            let b = s[i]; i += 1;
            let w = s[i] as i16 * 4; i += 1;
            let lval = if b & 1 != 0 { let v = s[i] as i8 as i16 * 8; i += 1; v } else { 0 };
            f(k, BaseLink { to: prev as u32, ty: b >> 1, w, lval });
            k += 1;
        }
    }
    pub fn degree(&self, u: u32) -> usize { if u as usize >= self.len() { return 0; } let mut d = 0; self.decode(u, |_, _| d += 1); d }
    pub fn base_link(&self, u: u32, k: usize) -> Option<BaseLink> {
        if u as usize >= self.len() { return None; }
        let mut out = None;
        self.decode(u, |j, l| if j == k { out = Some(l) });
        out
    }
    /// Shared bytes: offsets, stream, valence, tags, keys, anchors (names are debug only).
    pub fn bytes(&self) -> usize {
        self.offsets.len() * 4 + self.stream.len() + self.val.len() * 2 + self.tags.len() + self.keys.len() * 4 + self.anchors.len() * std::mem::size_of::<(u32, Anchor)>()
    }
}

// ------------------------------------------------------------------------------------- overlay

/// A link the mind added itself (from a base node or one of its own).
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct Added { pub src: u32, pub to: u32, pub ty: u8, pub w: i16, pub lval: i16 }

/// One mind's difference from its species base.
#[derive(Clone, Debug, Default, Hash, PartialEq, Eq)]
pub struct Overlay {
    /// Node valence, dense, for every node (base and own).
    pub val: Vec<i16>,
    /// The mind's anchors (where it believes its places and things are), sorted by node.
    pub anchors: Vec<(u32, Anchor)>,
    /// Slow weights of base links that differ from the base.
    pub slow: Sparse,
    /// Link valences of base links that differ from the base.
    pub lval: Sparse,
    /// Nodes the mind added: tags and stable key (valence is in `val`). Ids follow the base's.
    pub new_tags: Vec<u8>,
    pub new_keys: Vec<u32>,
    pub new_names: Vec<String>,
    /// Added links, sorted by source; among one source, in the order they were added.
    pub added: Vec<Added>,
}
impl Overlay {
    /// Bytes as stored (names are debug only): 2 per node of valence, anchors, 8 per sparse
    /// entry (as held), 5 per own node, 16 per added link (as held).
    pub fn bytes(&self) -> usize {
        self.val.len() * 2 + self.anchors.len() * std::mem::size_of::<(u32, Anchor)>() + self.slow.bytes() + self.lval.bytes()
            + self.new_tags.len() * 5 + self.added.len() * std::mem::size_of::<Added>()
    }
}

// ------------------------------------------------------------------------------------- runtime

/// The form a mind runs on: the shared species base, the mind's overlay, sparse fast weights.
#[derive(Clone, Debug)]
pub struct Runtime { pub base: Arc<Base>, pub ov: Overlay, pub fast: Sparse }

impl Runtime {
    /// A new mind of a species, exactly its founding graph.
    pub fn new(base: Arc<Base>) -> Runtime {
        let ov = Overlay { val: base.val.clone(), anchors: base.anchors.clone(), ..Default::default() };
        Runtime { base, ov, fast: Sparse::default() }
    }
    /// The index in `ov.added` of link `k` of `u`, which must be one of u's added links.
    fn added_index(&self, u: u32, k: usize) -> usize {
        let r = self.added_range(u);
        let j = k.checked_sub(self.base.degree(u)).expect("not an added link");
        assert!(j < r.len(), "link key past the end of node {u}'s links");
        r.start + j
    }
    fn added_range(&self, u: u32) -> std::ops::Range<usize> {
        let s = self.ov.added.partition_point(|a| a.src < u);
        let e = self.ov.added.partition_point(|a| a.src <= u);
        s..e
    }
    /// Expand into the inspectable form (every value spelled out).
    pub fn expand(&self) -> Expanded {
        let n = self.len();
        let mut e = Expanded { nbase: self.base.len(), offsets: vec![0], base_deg: vec![], val: self.ov.val.clone(), tags: vec![], keys: vec![], anchors: self.ov.anchors.clone(),
            names: vec![], targets: vec![], ltype: vec![], slow: vec![], lval: vec![], fast: vec![] };
        let mut ls = vec![];
        for u in 0..n as u32 {
            e.tags.push(self.tags(u)); e.keys.push(self.key(u)); e.names.push(self.name(u).to_string());
            e.base_deg.push(self.base.degree(u) as u32);
            ls.clear(); self.links(u, &mut ls);
            for l in &ls { e.targets.push(l.to); e.ltype.push(l.ty); e.slow.push(l.slow); e.lval.push(l.lval); e.fast.push(l.fast); }
            e.offsets.push(e.targets.len() as u32);
        }
        e
    }
    /// Fold this mind's whole overlay into a new base of its own (a recompile). Every link keeps
    /// its position, so fast weights stay on their links. Lossless only up to the base's
    /// quantisation (weights in steps of 4, link valence in steps of 8).
    pub fn recompile(&self, origin: u64) -> Runtime {
        let ex = self.expand();
        let mut b = Builder::new(origin);
        for u in 0..ex.len() { b.names.push(ex.names[u].clone()); b.val.push(ex.val[u]); b.tags.push(ex.tags[u]); b.keys.push(ex.keys[u]); }
        b.anchors = ex.anchors.clone();
        for u in 0..ex.len() as u32 { for l in ex.range(u) { b.links.push((u, ex.targets[l], ex.ltype[l], ex.slow[l], ex.lval[l])); } }
        let mut r = Runtime::new(Arc::new(Base::compile_ordered(b)));
        for (k, v) in self.fast.keys.iter().zip(&self.fast.vals) { r.fast.set(*k, Some(*v)); }
        r
    }
}

impl Form for Runtime {
    fn len(&self) -> usize { self.ov.val.len() }
    #[inline] fn val(&self, n: u32) -> i32 { self.ov.val[n as usize] as i32 }
    #[inline] fn tags(&self, n: u32) -> u8 { let b = &self.base; if (n as usize) < b.len() { b.tags[n as usize] } else { self.ov.new_tags[n as usize - b.len()] } }
    #[inline] fn key(&self, n: u32) -> u32 { let b = &self.base; if (n as usize) < b.len() { b.keys[n as usize] } else { self.ov.new_keys[n as usize - b.len()] } }
    #[inline]
    fn edges(&self, u: u32, out: &mut Vec<Edge>) {
        let (mut is, mut il, mut iff) = (self.ov.slow.start(u), self.ov.lval.start(u), self.fast.start(u));
        let (sk, lk, fk) = (&self.ov.slow.keys, &self.ov.lval.keys, &self.fast.keys);
        let mut deg = 0;
        if (u as usize) < self.base.len() {
            self.base.decode(u, |k, b| {
                let key = link_key(u, k);
                let mut w = b.w as i32;
                if is < sk.len() && sk[is] == key { w = self.ov.slow.vals[is] as i32; is += 1; }
                let mut val = b.lval as i32;
                if il < lk.len() && lk[il] == key { val = self.ov.lval.vals[il] as i32; il += 1; }
                if iff < fk.len() && fk[iff] == key { w += self.fast.vals[iff] as i32; iff += 1; }
                let w = w.min(1000);
                if w > 0 { out.push(Edge { to: b.to, w, ty: b.ty, val, link: key }); }
                deg = k + 1;
            });
        }
        for a in &self.ov.added[self.added_range(u)] {
            let key = link_key(u, deg);
            deg += 1;
            let mut w = a.w as i32;
            if iff < fk.len() && fk[iff] == key { w += self.fast.vals[iff] as i32; iff += 1; }
            let w = w.min(1000);
            if w > 0 { out.push(Edge { to: a.to, w, ty: a.ty, val: a.lval as i32, link: key }); }
        }
    }
}

impl Store for Runtime {
    fn name(&self, n: u32) -> &str { let b = &self.base; if (n as usize) < b.len() { &b.names[n as usize] } else { &self.ov.new_names[n as usize - b.len()] } }
    fn set_val(&mut self, n: u32, v: i16) { self.ov.val[n as usize] = v; }
    fn anchor(&self, n: u32) -> Option<Anchor> { self.ov.anchors.binary_search_by_key(&n, |a| a.0).ok().map(|i| self.ov.anchors[i].1) }
    fn set_anchor(&mut self, n: u32, a: Anchor) {
        match self.ov.anchors.binary_search_by_key(&n, |a| a.0) { Ok(i) => self.ov.anchors[i].1 = a, Err(i) => self.ov.anchors.insert(i, (n, a)) }
    }
    fn links(&self, u: u32, out: &mut Vec<Link>) {
        let mut deg = 0;
        if (u as usize) < self.base.len() {
            self.base.decode(u, |k, b| {
                let key = link_key(u, k);
                out.push(Link { key, to: b.to, ty: b.ty, slow: self.ov.slow.get(key).unwrap_or(b.w), lval: self.ov.lval.get(key).unwrap_or(b.lval), fast: self.fast.get(key).unwrap_or(0) });
                deg = k + 1;
            });
        }
        for a in &self.ov.added[self.added_range(u)] {
            let key = link_key(u, deg);
            deg += 1;
            out.push(Link { key, to: a.to, ty: a.ty, slow: a.w, lval: a.lval, fast: self.fast.get(key).unwrap_or(0) });
        }
    }
    fn link(&self, key: LinkKey) -> Link {
        let (u, k) = (link_src(key), link_pos(key));
        let fast = self.fast.get(key).unwrap_or(0);
        if let Some(b) = self.base.base_link(u, k) {
            return Link { key, to: b.to, ty: b.ty, slow: self.ov.slow.get(key).unwrap_or(b.w), lval: self.ov.lval.get(key).unwrap_or(b.lval), fast };
        }
        let a = self.ov.added[self.added_index(u, k)];
        Link { key, to: a.to, ty: a.ty, slow: a.w, lval: a.lval, fast }
    }
    fn set_slow(&mut self, key: LinkKey, w: i16) {
        let (u, k) = (link_src(key), link_pos(key));
        match self.base.base_link(u, k) {
            Some(b) => self.ov.slow.set(key, if w == b.w { None } else { Some(w) }),
            None => { let i = self.added_index(u, k); self.ov.added[i].w = w; }
        }
    }
    fn set_lval(&mut self, key: LinkKey, v: i16) {
        let (u, k) = (link_src(key), link_pos(key));
        match self.base.base_link(u, k) {
            Some(b) => self.ov.lval.set(key, if v == b.lval { None } else { Some(v) }),
            None => { let i = self.added_index(u, k); self.ov.added[i].lval = v; }
        }
    }
    fn set_fast(&mut self, key: LinkKey, f: i16) { self.fast.set(key, if f == 0 { None } else { Some(f) }); }
    fn fast_entries(&self) -> Vec<(LinkKey, i16)> { self.fast.keys.iter().copied().zip(self.fast.vals.iter().copied()).collect() }
    fn map_fast(&mut self, f: &mut dyn FnMut(i16) -> i16) {
        let mut i = 0;
        while i < self.fast.keys.len() {
            let v = f(self.fast.vals[i]);
            if v == 0 { self.fast.keys.remove(i); self.fast.vals.remove(i); } else { self.fast.vals[i] = v; i += 1; }
        }
    }
    fn fast_total(&self) -> i64 { self.fast.vals.iter().map(|&f| (f as i64).abs()).sum() }
    fn add_node(&mut self, name: &str, val: i32, tags: u8, anchor: Option<Anchor>, origin: u64) -> u32 {
        let id = self.len() as u32;
        self.ov.val.push(val.clamp(-1000, 1000) as i16);
        self.ov.new_tags.push(tags | if anchor.is_some() { tag::ANCHORED } else { 0 });
        self.ov.new_keys.push(node_key(origin, id as u64));
        self.ov.new_names.push(name.to_string());
        if let Some(a) = anchor { self.set_anchor(id, a); }
        id
    }
    fn add_link(&mut self, u: u32, v: u32, ty: u8, w: i32, lval: i32) -> LinkKey {
        let r = self.added_range(u);
        let k = self.base.degree(u) + r.len();
        assert!(k < 1 << LINK_BITS);
        self.ov.added.insert(r.end, Added { src: u, to: v, ty, w: w.clamp(0, 1000) as i16, lval: lval.clamp(-1000, 1000) as i16 });
        link_key(u, k)
    }
    fn bytes(&self) -> Bytes { Bytes { shared: self.base.bytes(), own: self.ov.bytes(), fast: self.fast.bytes() } }
    fn link_count(&self) -> usize { self.base.links + self.ov.added.len() }
    fn base_degree(&self, u: u32) -> usize { self.base.degree(u) }
    fn own_sources(&self) -> Vec<u32> { let mut v: Vec<u32> = self.ov.added.iter().map(|a| a.src).collect(); v.dedup(); v }
    fn remove_link(&mut self, key: LinkKey) {
        let (u, k) = (link_src(key), link_pos(key));
        let i = self.added_index(u, k);
        self.ov.added.remove(i);
        let (s, e) = (self.fast.start(u), self.fast.start(u + 1));
        let mut moved = Sparse::default();
        for j in s..e {
            let (kk, v) = (self.fast.keys[j], self.fast.vals[j]);
            if link_pos(kk) == k { continue; }
            moved.keys.push(if link_pos(kk) > k { kk - 1 } else { kk }); moved.vals.push(v);
        }
        self.fast.keys.splice(s..e, moved.keys);
        self.fast.vals.splice(s..e, moved.vals);
    }
}

// ------------------------------------------------------------------------------------- expanded

/// The inspectable form: plain CSR, every value spelled out (i16 weights, dense fast weights).
#[derive(Clone, Debug, Default, Hash, PartialEq, Eq)]
pub struct Expanded {
    /// Nodes `0..nbase` came from the species base; the first `base_deg[u]` links of each node did.
    pub nbase: usize,
    pub base_deg: Vec<u32>,
    pub offsets: Vec<u32>,
    pub val: Vec<i16>,
    pub tags: Vec<u8>,
    pub keys: Vec<u32>,
    pub anchors: Vec<(u32, Anchor)>,
    pub names: Vec<String>,
    pub targets: Vec<u32>,
    pub ltype: Vec<u8>,
    pub slow: Vec<i16>,
    pub lval: Vec<i16>,
    pub fast: Vec<i16>,
}

impl Expanded {
    #[inline] pub fn range(&self, u: u32) -> std::ops::Range<usize> { self.offsets[u as usize] as usize..self.offsets[u as usize + 1] as usize }
    #[inline] fn idx(&self, key: LinkKey) -> usize {
        let r = self.range(link_src(key));
        assert!(link_pos(key) < r.len(), "link key past the end of its node's links");
        r.start + link_pos(key)
    }
    /// Compile back into the runtime form over a species base: whatever differs from the base
    /// goes into the overlay. Panics if this mind isn't built on that base.
    pub fn compile(&self, base: &Arc<Base>) -> Runtime {
        assert_eq!(self.nbase, base.len());
        let mut r = Runtime::new(base.clone());
        r.ov.val = self.val.clone();
        r.ov.anchors = self.anchors.clone();
        for u in 0..self.len() as u32 {
            let rg = self.range(u);
            let bd = self.base_deg[u as usize] as usize;
            if (u as usize) < self.nbase {
                assert_eq!(bd, base.degree(u));
                base.decode(u, |k, b| {
                    let l = rg.start + k;
                    assert!(self.targets[l] == b.to && self.ltype[l] == b.ty, "not this base's link");
                    if self.slow[l] != b.w { r.ov.slow.set(link_key(u, k), Some(self.slow[l])); }
                    if self.lval[l] != b.lval { r.ov.lval.set(link_key(u, k), Some(self.lval[l])); }
                });
            } else {
                r.ov.new_tags.push(self.tags[u as usize]); r.ov.new_keys.push(self.keys[u as usize]); r.ov.new_names.push(self.names[u as usize].clone());
            }
            for l in rg.start + bd..rg.end { r.ov.added.push(Added { src: u, to: self.targets[l], ty: self.ltype[l], w: self.slow[l], lval: self.lval[l] }); }
            for (k, l) in rg.enumerate() { if self.fast[l] != 0 { r.fast.set(link_key(u, k), Some(self.fast[l])); } }
        }
        r
    }
    /// Bytes counted as the thought graph counted its graph form (no allocator slack or names).
    pub fn graph_bytes(&self) -> usize {
        self.val.len() * 2 + self.tags.len() + self.keys.len() * 4 + self.anchors.len() * std::mem::size_of::<(u32, Anchor)>()
            + self.offsets.len() * 4 + self.targets.len() * 4 + self.slow.len() * 2 + self.lval.len() * 2 + self.ltype.len()
    }
}

impl Form for Expanded {
    fn len(&self) -> usize { self.val.len() }
    #[inline] fn val(&self, n: u32) -> i32 { self.val[n as usize] as i32 }
    #[inline] fn tags(&self, n: u32) -> u8 { self.tags[n as usize] }
    #[inline] fn key(&self, n: u32) -> u32 { self.keys[n as usize] }
    #[inline]
    fn edges(&self, u: u32, out: &mut Vec<Edge>) {
        let r = self.range(u);
        for (k, l) in r.enumerate() {
            let w = (self.slow[l] as i32 + self.fast[l] as i32).min(1000);
            if w > 0 { out.push(Edge { to: self.targets[l], w, ty: self.ltype[l], val: self.lval[l] as i32, link: link_key(u, k) }); }
        }
    }
}

impl Store for Expanded {
    fn name(&self, n: u32) -> &str { &self.names[n as usize] }
    fn set_val(&mut self, n: u32, v: i16) { self.val[n as usize] = v; }
    fn anchor(&self, n: u32) -> Option<Anchor> { self.anchors.binary_search_by_key(&n, |a| a.0).ok().map(|i| self.anchors[i].1) }
    fn set_anchor(&mut self, n: u32, a: Anchor) {
        match self.anchors.binary_search_by_key(&n, |a| a.0) { Ok(i) => self.anchors[i].1 = a, Err(i) => self.anchors.insert(i, (n, a)) }
    }
    fn links(&self, u: u32, out: &mut Vec<Link>) {
        for (k, l) in self.range(u).enumerate() {
            out.push(Link { key: link_key(u, k), to: self.targets[l], ty: self.ltype[l], slow: self.slow[l], lval: self.lval[l], fast: self.fast[l] });
        }
    }
    fn link(&self, key: LinkKey) -> Link {
        let l = self.idx(key);
        Link { key, to: self.targets[l], ty: self.ltype[l], slow: self.slow[l], lval: self.lval[l], fast: self.fast[l] }
    }
    fn set_slow(&mut self, key: LinkKey, w: i16) { let l = self.idx(key); self.slow[l] = w; }
    fn set_lval(&mut self, key: LinkKey, v: i16) { let l = self.idx(key); self.lval[l] = v; }
    fn set_fast(&mut self, key: LinkKey, f: i16) { let l = self.idx(key); self.fast[l] = f; }
    fn fast_entries(&self) -> Vec<(LinkKey, i16)> {
        let mut v = vec![];
        for u in 0..self.len() as u32 { for (k, l) in self.range(u).enumerate() { if self.fast[l] != 0 { v.push((link_key(u, k), self.fast[l])); } } }
        v
    }
    fn map_fast(&mut self, f: &mut dyn FnMut(i16) -> i16) { for x in self.fast.iter_mut().filter(|x| **x != 0) { *x = f(*x); } }
    fn fast_total(&self) -> i64 { self.fast.iter().map(|&f| (f as i64).abs()).sum() }
    fn add_node(&mut self, name: &str, val: i32, tags: u8, anchor: Option<Anchor>, origin: u64) -> u32 {
        let id = self.len() as u32;
        self.val.push(val.clamp(-1000, 1000) as i16);
        self.tags.push(tags | if anchor.is_some() { tag::ANCHORED } else { 0 });
        self.keys.push(node_key(origin, id as u64));
        self.names.push(name.to_string());
        self.base_deg.push(0);
        let end = *self.offsets.last().unwrap();
        self.offsets.push(end);
        if let Some(a) = anchor { self.set_anchor(id, a); }
        id
    }
    fn add_link(&mut self, u: u32, v: u32, ty: u8, w: i32, lval: i32) -> LinkKey {
        let at = self.offsets[u as usize + 1] as usize;
        let k = at - self.offsets[u as usize] as usize;
        self.targets.insert(at, v); self.ltype.insert(at, ty); self.slow.insert(at, w.clamp(0, 1000) as i16);
        self.lval.insert(at, lval.clamp(-1000, 1000) as i16); self.fast.insert(at, 0);
        for o in &mut self.offsets[u as usize + 1..] { *o += 1; }
        link_key(u, k)
    }
    fn bytes(&self) -> Bytes { Bytes { shared: 0, own: self.graph_bytes(), fast: self.fast.len() * 2 } }
    fn link_count(&self) -> usize { self.targets.len() }
    fn base_degree(&self, u: u32) -> usize { self.base_deg[u as usize] as usize }
    fn remove_link(&mut self, key: LinkKey) {
        let u = link_src(key);
        assert!(link_pos(key) >= self.base_deg[u as usize] as usize, "not one of the mind's own links");
        let l = self.idx(key);
        self.targets.remove(l); self.ltype.remove(l); self.slow.remove(l); self.lval.remove(l); self.fast.remove(l);
        for o in &mut self.offsets[u as usize + 1..] { *o -= 1; }
    }
}
