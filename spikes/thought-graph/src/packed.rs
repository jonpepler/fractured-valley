//! A second compiled form, for comparison: the graph quantised and baked. Slow and fast weights
//! are summed and stored in one byte (steps of 4), link valence and debug names are dropped,
//! node valence is one byte (steps of 8), and each link's type sits in its own byte. Thinking is
//! the same CSR walk. It answers "how small can the readable form get", not "how opaque".

use crate::graph::{Anchor, Graph};
use crate::spread::{Edge, Form};

#[derive(Clone, Debug)]
pub struct Packed {
    pub offsets: Vec<u32>,
    pub targets: Vec<u32>,
    /// Weight / 4 (0..250).
    pub w: Vec<u8>,
    pub ty: Vec<u8>,
    /// Valence / 8.
    pub val: Vec<i8>,
    pub tags: Vec<u8>,
    pub anchors: Vec<(u32, Anchor)>,
}

impl Packed {
    pub fn compile<F: Form>(f: &F, g: &Graph) -> Packed {
        let n = f.len();
        let (mut offsets, mut targets, mut w, mut ty) = (vec![0u32], vec![], vec![], vec![]);
        let mut edges = vec![];
        for u in 0..n as u32 {
            edges.clear();
            f.edges(u, &mut edges);
            for e in &edges { targets.push(e.to); w.push((e.w / 4).clamp(0, 250) as u8); ty.push(e.ty); }
            offsets.push(targets.len() as u32);
        }
        Packed { offsets, targets, w, ty, val: (0..n as u32).map(|i| (f.val(i) / 8).clamp(-127, 127) as i8).collect(),
            tags: (0..n as u32).map(|i| f.tags(i)).collect(), anchors: g.anchors.clone() }
    }
    pub fn bytes(&self) -> usize {
        self.offsets.len() * 4 + self.targets.len() * 4 + self.w.len() + self.ty.len() + self.val.len() + self.tags.len()
            + self.anchors.len() * std::mem::size_of::<(u32, Anchor)>()
    }
}

impl Form for Packed {
    fn len(&self) -> usize { self.val.len() }
    fn val(&self, n: u32) -> i32 { self.val[n as usize] as i32 * 8 }
    fn tags(&self, n: u32) -> u8 { self.tags[n as usize] }
    fn edges(&self, u: u32, out: &mut Vec<Edge>) {
        for l in self.offsets[u as usize] as usize..self.offsets[u as usize + 1] as usize {
            let w = self.w[l] as i32 * 4;
            if w > 0 { out.push(Edge { to: self.targets[l], w, ty: self.ty[l], val: 0, link: u32::MAX }); }
        }
    }
}
