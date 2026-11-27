//! Shared vocabulary of every mind: link types, node tags, anchors, link keys, and the builder a
//! species uses to author its founding graph. Copied from `spikes/thought-graph` (`graph.rs`),
//! with two additions: every node has a **stable key** (ties are broken by it, never by the
//! array index), and links are addressed by a [`LinkKey`] (source, position) that both forms of a
//! mind share.

/// Link types. Modulators scale some of them (see `think::Gain`).
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

/// A link's address in either form of a mind: its source node and its position among that
/// node's links (the species base's links first, in base order, then the mind's own added links).
pub type LinkKey = u64;
pub const LINK_BITS: u32 = 16;
#[inline] pub fn link_key(u: u32, k: usize) -> LinkKey { ((u as u64) << LINK_BITS) | k as u64 }
#[inline] pub fn link_src(key: LinkKey) -> u32 { (key >> LINK_BITS) as u32 }
#[inline] pub fn link_pos(key: LinkKey) -> usize { (key & ((1 << LINK_BITS) - 1)) as usize }

/// The stable key of a node: a hash of where it came from (its species and the order it was
/// authored in, or the mind and the order it was learnt in), not of its position in any array.
pub fn node_key(origin: u64, index: u64) -> u32 { crate::rng::hash(origin, index, 0x6e0d_e4e7, 0) as u32 }

/// Collects nodes and links, then packs them (into a species base or an expanded mind).
#[derive(Default, Clone)]
pub struct Builder {
    pub names: Vec<String>,
    pub val: Vec<i16>,
    pub tags: Vec<u8>,
    pub keys: Vec<u32>,
    pub anchors: Vec<(u32, Anchor)>,
    /// (source, target, type, weight, link valence)
    pub links: Vec<(u32, u32, u8, i16, i16)>,
    /// Seed of the stable node keys (the species).
    pub origin: u64,
}

impl Builder {
    pub fn new(origin: u64) -> Builder { Builder { origin, ..Default::default() } }
    pub fn node(&mut self, name: &str, val: i32, tags: u8, anchor: Option<Anchor>) -> u32 {
        let id = self.val.len() as u32;
        self.names.push(name.into());
        self.val.push(val.clamp(-1000, 1000) as i16);
        self.tags.push(tags | if anchor.is_some() { tag::ANCHORED } else { 0 });
        self.keys.push(node_key(self.origin, id as u64));
        if let Some(a) = anchor { self.anchors.push((id, a)); }
        id
    }
    pub fn link(&mut self, s: u32, t: u32, ty: u8, w: i32) { self.links.push((s, t, ty, w.clamp(0, 1000) as i16, 0)); }
    pub fn link_v(&mut self, s: u32, t: u32, ty: u8, w: i32, v: i32) { self.links.push((s, t, ty, w.clamp(0, 1000) as i16, v.clamp(-1000, 1000) as i16)); }
    pub fn both(&mut self, a: u32, b: u32, ty: u8, ab: i32, ba: i32) { self.link(a, b, ty, ab); self.link(b, a, ty, ba); }
    /// Links sorted by (source, target, type); a repeated (source, target, type) keeps the larger
    /// weight. Anchors sorted by node.
    pub fn canonical(&mut self) {
        self.links.sort_by_key(|l| (l.0, l.1, l.2, std::cmp::Reverse(l.3), l.4));
        self.links.dedup_by_key(|l| (l.0, l.1, l.2));
        self.anchors.sort_by_key(|a| a.0);
    }
    pub fn len(&self) -> usize { self.val.len() }
    pub fn is_empty(&self) -> bool { self.val.is_empty() }
}
