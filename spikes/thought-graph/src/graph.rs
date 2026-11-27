//! The inspectable form of a mind: a packed graph in compressed sparse rows (CSR).
//!
//! No node or link objects: a node is an index into parallel arrays, and a node's out-links are
//! the slice `offsets[n]..offsets[n + 1]` of the link arrays. Node names are kept only as a
//! debug table; nothing in thinking reads them.

/// Link types. Modulators scale some of them (see `spread::gain`).
pub mod lt {
    /// A body state wants something: hunger → food.
    pub const WANTS: u8 = 0;
    /// Instance and kind: the hens ↔ food, the dog ↔ danger.
    pub const KIND: u8 = 1;
    /// A place holds a thing.
    pub const HOLDS: u8 = 2;
    /// A thing is at a place.
    pub const AT: u8 = 3;
    /// Two places are physically close (weight from real distance).
    pub const NEAR: u8 = 4;
    /// Two places are alike: they hold the same kinds of thing.
    pub const LIKE: u8 = 5;
    /// An association learned from experience or a feeling (starts at weight 0 as an empty slot).
    pub const FEELS: u8 = 6;
    /// A plain association between concepts.
    pub const ASSOC: u8 = 7;
    pub const COUNT: usize = 8;
    pub const NAMES: [&str; COUNT] = ["wants", "kind", "holds", "at", "near", "like", "feels", "assoc"];
}

/// Node tags (a bit set). `ANCHORED` nodes carry a world reference; the rest are open connectors.
pub mod tag {
    pub const FOOD: u8 = 1;
    pub const REST: u8 = 2;
    pub const PLACE: u8 = 4;
    pub const BODY: u8 = 8;
    pub const DANGER: u8 = 16;
    pub const CREATURE: u8 = 32;
    pub const ANCHORED: u8 = 64;
    /// Anger and aggression: favoured under stress.
    pub const ANGER: u8 = 128;
}

/// What an anchored node is tied to in the world.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Anchor {
    /// A place with real coordinates (cells).
    Place { x: i16, y: i16 },
    /// A physical thing or creature, by entity id, with where it was last seen.
    Entity { id: u32, x: i16, y: i16 },
}
impl Anchor {
    pub fn xy(self) -> (i32, i32) {
        match self { Anchor::Place { x, y } | Anchor::Entity { x, y, .. } => (x as i32, y as i32) }
    }
}

#[derive(Clone, Debug, Default, Hash)]
pub struct Graph {
    /// Node valence, -1000..1000.
    pub val: Vec<i16>,
    pub tags: Vec<u8>,
    /// Anchors, sorted by node id (sparse: most nodes in a big mind are connectors).
    pub anchors: Vec<(u32, Anchor)>,
    pub offsets: Vec<u32>,
    pub targets: Vec<u32>,
    /// Slow (long-term) link weights, 0..1000.
    pub slow: Vec<i16>,
    /// Link valence, -1000..1000: how traversing this link feels, on top of the target's valence.
    pub lval: Vec<i16>,
    pub ltype: Vec<u8>,
    /// Debug labels only.
    pub names: Vec<String>,
}

impl Graph {
    pub fn len(&self) -> usize { self.val.len() }
    pub fn link_count(&self) -> usize { self.targets.len() }
    pub fn links(&self, n: u32) -> std::ops::Range<usize> { self.offsets[n as usize] as usize..self.offsets[n as usize + 1] as usize }
    pub fn anchor(&self, n: u32) -> Option<Anchor> {
        self.anchors.binary_search_by_key(&n, |a| a.0).ok().map(|i| self.anchors[i].1)
    }
    pub fn set_anchor(&mut self, n: u32, a: Anchor) {
        if let Ok(i) = self.anchors.binary_search_by_key(&n, |a| a.0) { self.anchors[i].1 = a; }
    }
    /// The source node of a link (a binary search over offsets).
    pub fn source(&self, l: usize) -> u32 { (self.offsets.partition_point(|&o| o as usize <= l) - 1) as u32 }
    /// The first link u → v of a type, if any.
    pub fn find(&self, u: u32, v: u32, ty: u8) -> Option<usize> {
        self.links(u).find(|&l| self.targets[l] == v && self.ltype[l] == ty)
    }
    pub fn id(&self, name: &str) -> u32 { self.names.iter().position(|n| n == name).unwrap_or_else(|| panic!("no node {name}")) as u32 }

    /// Bytes of the structure, counted by array (no allocator slack, no debug names).
    pub fn bytes(&self) -> usize {
        self.val.len() * 2 + self.tags.len() + self.anchors.len() * std::mem::size_of::<(u32, Anchor)>()
            + self.offsets.len() * 4 + self.targets.len() * 4 + self.slow.len() * 2 + self.lval.len() * 2 + self.ltype.len()
    }
}

/// Collects nodes and links, then packs them.
#[derive(Default)]
pub struct Builder {
    pub names: Vec<String>,
    pub val: Vec<i16>,
    pub tags: Vec<u8>,
    pub anchors: Vec<(u32, Anchor)>,
    /// (source, target, type, weight, link valence)
    pub links: Vec<(u32, u32, u8, i16, i16)>,
}

impl Builder {
    pub fn node(&mut self, name: &str, val: i32, tags: u8, anchor: Option<Anchor>) -> u32 {
        let id = self.val.len() as u32;
        self.names.push(name.into());
        self.val.push(val as i16);
        self.tags.push(tags | if anchor.is_some() { tag::ANCHORED } else { 0 });
        if let Some(a) = anchor { self.anchors.push((id, a)); }
        id
    }
    pub fn link(&mut self, s: u32, t: u32, ty: u8, w: i32) { self.links.push((s, t, ty, w.clamp(0, 1000) as i16, 0)); }
    pub fn both(&mut self, a: u32, b: u32, ty: u8, ab: i32, ba: i32) { self.link(a, b, ty, ab); self.link(b, a, ty, ba); }
    /// Pack into CSR. Links are sorted by (source, target, type); a repeated (source, target, type)
    /// keeps the larger weight.
    pub fn build(mut self) -> Graph {
        self.links.sort_by_key(|l| (l.0, l.1, l.2, std::cmp::Reverse(l.3)));
        self.links.dedup_by_key(|l| (l.0, l.1, l.2));
        self.anchors.sort_by_key(|a| a.0);
        let n = self.val.len();
        let mut offsets = vec![0u32; n + 1];
        for l in &self.links { offsets[l.0 as usize + 1] += 1; }
        for i in 0..n { offsets[i + 1] += offsets[i]; }
        Graph {
            val: self.val, tags: self.tags, anchors: self.anchors, offsets,
            targets: self.links.iter().map(|l| l.1).collect(),
            slow: self.links.iter().map(|l| l.3).collect(),
            lval: self.links.iter().map(|l| l.4).collect(),
            ltype: self.links.iter().map(|l| l.2).collect(),
            names: self.names,
        }
    }
}
