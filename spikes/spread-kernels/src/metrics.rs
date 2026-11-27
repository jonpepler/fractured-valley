//! Distance of a redefined kernel from the oracle, along each instance's own trajectory.

/// Fixed downstream linear scores: K weight vectors over node ids, from a hash (-100..=100).
pub const K: u32 = 4;
#[inline]
pub fn weight(k: u32, n: u32) -> i64 {
    let mut z = (n as u64) << 8 | k as u64;
    z = (z ^ (z >> 33)).wrapping_mul(0xff51_afd7_ed55_8ccd); z ^= z >> 33;
    (z % 201) as i64 - 100
}
pub fn argmax(st: &[(u32, i32)]) -> u32 {
    let mut best = (i64::MIN, 0);
    for k in 0..K { let s: i64 = st.iter().map(|&(n, x)| x as i64 * weight(k, n)).sum(); if s > best.0 { best = (s, k); } }
    best.1
}

#[derive(Default, Clone, Copy, Debug)]
pub struct Dist { pub steps: u64, pub top_overlap: f64, pub top_exact: u64, pub l1_num: f64, pub l1_den: f64, pub argmax_diff: u64 }
impl Dist {
    /// `a` is the oracle state (sorted by node, x > 0), `b` the candidate's.
    pub fn add(&mut self, a: &[(u32, i32)], oa: &[u32], b: &[(u32, i32)], ob: &[u32]) {
        self.steps += 1;
        let common = oa.iter().filter(|n| ob.contains(n)).count();
        self.top_overlap += common as f64 / oa.len().max(1) as f64;
        if oa == ob { self.top_exact += 1; }
        let (mut i, mut j) = (0, 0);
        while i < a.len() || j < b.len() {
            let (na, nb) = (a.get(i).map_or(u32::MAX, |x| x.0), b.get(j).map_or(u32::MAX, |x| x.0));
            if na == nb { self.l1_num += (a[i].1 - b[j].1).abs() as f64; i += 1; j += 1; }
            else if na < nb { self.l1_num += a[i].1 as f64; i += 1; } else { self.l1_num += b[j].1 as f64; j += 1; }
        }
        self.l1_den += a.iter().map(|x| x.1 as f64).sum::<f64>();
        if argmax(a) != argmax(b) { self.argmax_diff += 1; }
    }
    pub fn merge(&mut self, o: &Dist) { self.steps += o.steps; self.top_overlap += o.top_overlap; self.top_exact += o.top_exact; self.l1_num += o.l1_num; self.l1_den += o.l1_den; self.argmax_diff += o.argmax_diff; }
    pub fn top8(&self) -> f64 { self.top_overlap / self.steps as f64 }
    pub fn top8_exact(&self) -> f64 { self.top_exact as f64 / self.steps as f64 }
    pub fn rel_l1(&self) -> f64 { self.l1_num / self.l1_den }
    pub fn argmax_rate(&self) -> f64 { self.argmax_diff as f64 / self.steps as f64 }
}
