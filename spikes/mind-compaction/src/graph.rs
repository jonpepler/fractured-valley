//! The reference form of a mind: the thought-graph spike's CSR graph (`spikes/thought-graph`,
//! `graph.rs`), copied and trimmed (no debug names). A node is an index into parallel arrays; a
//! node's out-links are the slice `offsets[n]..offsets[n + 1]` of the link arrays.

/// Link types, as in the thought graph.
pub mod lt {
    pub const WANTS: u8 = 0;
    pub const KIND: u8 = 1;
    pub const HOLDS: u8 = 2;
    pub const AT: u8 = 3;
    pub const NEAR: u8 = 4;
    pub const LIKE: u8 = 5;
    pub const FEELS: u8 = 6;
    pub const ASSOC: u8 = 7;
    pub const COUNT: usize = 8;
}

/// Node tags (a bit set), as in the thought graph.
pub mod tag {
    pub const FOOD: u8 = 1;
    pub const REST: u8 = 2;
    pub const PLACE: u8 = 4;
    pub const BODY: u8 = 8;
    pub const DANGER: u8 = 16;
    pub const CREATURE: u8 = 32;
    pub const ANCHORED: u8 = 64;
    pub const ANGER: u8 = 128;
}

/// Where an anchored node is in the world (cells). Places and things alike.
pub type Anchor = (u32, i16, i16);

#[derive(Clone, Debug, Default, Hash)]
pub struct Graph {
    /// Node valence, -1000..1000.
    pub val: Vec<i16>,
    pub tags: Vec<u8>,
    /// Anchors (node, x, y), sorted by node id.
    pub anchors: Vec<Anchor>,
    pub offsets: Vec<u32>,
    pub targets: Vec<u32>,
    /// Slow link weights, 0..1000.
    pub slow: Vec<i16>,
    /// Link valence, -1000..1000.
    pub lval: Vec<i16>,
    pub ltype: Vec<u8>,
}

impl Graph {
    pub fn len(&self) -> usize { self.val.len() }
    pub fn is_empty(&self) -> bool { self.val.is_empty() }
    pub fn link_count(&self) -> usize { self.targets.len() }
    #[inline]
    pub fn links(&self, n: u32) -> std::ops::Range<usize> { self.offsets[n as usize] as usize..self.offsets[n as usize + 1] as usize }
    pub fn anchor(&self, n: u32) -> Option<(i16, i16)> {
        self.anchors.binary_search_by_key(&n, |a| a.0).ok().map(|i| (self.anchors[i].1, self.anchors[i].2))
    }
    pub fn find(&self, u: u32, v: u32, ty: u8) -> Option<usize> {
        self.links(u).find(|&l| self.targets[l] == v && self.ltype[l] == ty)
    }
    /// Bytes of the structure, counted by array, as the thought-graph spike counts them.
    pub fn bytes(&self) -> usize {
        self.val.len() * 2 + self.tags.len() + self.anchors.len() * std::mem::size_of::<Anchor>()
            + self.offsets.len() * 4 + self.targets.len() * 4 + self.slow.len() * 2 + self.lval.len() * 2 + self.ltype.len()
    }
    /// All links as builder tuples (source, target, type, weight, valence).
    pub fn link_tuples(&self) -> Vec<(u32, u32, u8, i16, i16)> {
        let mut v = Vec::with_capacity(self.link_count());
        for u in 0..self.len() as u32 {
            for l in self.links(u) { v.push((u, self.targets[l], self.ltype[l], self.slow[l], self.lval[l])); }
        }
        v
    }
    /// The same mind with node ids renumbered: node `i` becomes `perm[i]`.
    pub fn permute(&self, perm: &[u32]) -> Graph {
        let n = self.len();
        let mut b = Builder::default();
        let mut inv = vec![0u32; n];
        for (i, &p) in perm.iter().enumerate() { inv[p as usize] = i as u32; }
        for &old in &inv {
            let a = self.anchor(old);
            b.node(self.val[old as usize] as i32, self.tags[old as usize] & !tag::ANCHORED, a);
        }
        b.links = self.link_tuples().into_iter().map(|(s, t, ty, w, v)| (perm[s as usize], perm[t as usize], ty, w, v)).collect();
        b.build()
    }
}

/// Collects nodes and links, then packs them into CSR.
#[derive(Default, Clone)]
pub struct Builder {
    pub val: Vec<i16>,
    pub tags: Vec<u8>,
    pub anchors: Vec<Anchor>,
    /// (source, target, type, weight, link valence)
    pub links: Vec<(u32, u32, u8, i16, i16)>,
}

impl Builder {
    pub fn node(&mut self, val: i32, tags: u8, anchor: Option<(i16, i16)>) -> u32 {
        let id = self.val.len() as u32;
        self.val.push(val.clamp(-1000, 1000) as i16);
        self.tags.push(tags | if anchor.is_some() { tag::ANCHORED } else { 0 });
        if let Some((x, y)) = anchor { self.anchors.push((id, x, y)); }
        id
    }
    pub fn link(&mut self, s: u32, t: u32, ty: u8, w: i32, v: i32) {
        self.links.push((s, t, ty, w.clamp(0, 1000) as i16, v.clamp(-1000, 1000) as i16));
    }
    /// Pack into CSR. Links are sorted by (source, target, type); a repeated (source, target,
    /// type) keeps the larger weight.
    pub fn build(mut self) -> Graph {
        self.links.sort_unstable_by_key(|l| (l.0, l.1, l.2, std::cmp::Reverse(l.3), l.4));
        self.links.dedup_by_key(|l| (l.0, l.1, l.2));
        self.anchors.sort_unstable_by_key(|a| a.0);
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
        }
    }
}
