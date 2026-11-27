//! Heat across a coarse–fine boundary with exact books (Berger–Colella).
//!
//!     cargo run --release --bin reflux
//!
//! An 8 m cube of stone in 1 m coarse cells. Some coarse cells are refined
//! into 10 × 10 × 10 fine cells of 10 cm (refinement ratio 10). The fine
//! level takes 10 steps for each coarse step (subcycling). Energy is whole
//! µJ, temperature whole mK; every face flux is computed once and applied
//! with opposite signs, so the books can only break at the coarse–fine
//! boundary, and that is what this tests.
//!
//! Per coarse step, Berger–Colella order:
//! 1. Coarse step: every coarse face, including coarse–fine faces (the
//!    covered coarse cell holds the sum of its fine cells).
//! 2. Fine substeps: fine faces; at a coarse–fine face the coarse side is a
//!    ghost temperature interpolated in time between the coarse cell's old
//!    and new value. Each such fine flux is added to a flux register.
//! 3. Reflux: each uncovered coarse cell beside the fine region gets back
//!    the coarse flux it was charged and is charged the register's sum
//!    instead.
//! 4. Restrict: each covered coarse cell = the sum of its fine cells.
//!
//! Regridding: the refined set moves during the run (a "fire" travels).
//! Refining splits a coarse cell's energy into 1000 fine cells, linearly
//! (slopes from the coarse neighbours, limited) with the integer remainder
//! placed so the sum is exact; coarsening sums them.
//!
//! Runs compared: all fine (reference), adaptive with refluxing, adaptive
//! without refluxing, all coarse.
use std::collections::BTreeSet;

const NC: usize = 8; // coarse cells per side (1 m each)
const R: usize = 10; // refinement ratio (10 cm fine cells)
const NF: usize = NC * R;
const CF: i64 = 2_000_000; // fine cell heat capacity, µJ/mK (2 MJ/m³K stone, 1 L)
const CC: i64 = CF * (R * R * R) as i64; // coarse cell, µJ/mK
const K_MW: i64 = 2_000; // conductivity, mW/(m·K)
const DT_F: i64 = 400_000; // fine step, ms (400 s; stable limit ~1 670 s)
const DT_C: i64 = DT_F * R as i64; // coarse step
const AMBIENT: i64 = 290_000; // mK

/// Heat crossing a face in one step, µJ, for a temperature difference in mK:
/// k·A/d · ΔT · dt = K_MW·A_mm²·dt_s·ΔT_mK / (d_mm·1000). Truncates toward
/// zero, so flux(−ΔT) = −flux(ΔT): a face gives exactly what it takes.
#[derive(Clone, Copy)]
struct Face { num: i64, den: i64 }
const fn face(a_mm2: i64, d_mm: i64, dt_ms: i64) -> Face { Face { num: K_MW * a_mm2 * (dt_ms / 1000), den: d_mm * 1000 } }
fn flux(f: Face, dt_mk: i64) -> i64 {
    if f.num % f.den == 0 { if let Some(v) = dt_mk.checked_mul(f.num / f.den) { return v } }
    ((f.num as i128 * dt_mk as i128) / f.den as i128) as i64
}
// Face geometry, in mm: fine face 100² over 100; coarse 1000² over 1000;
// coarse–fine (per fine face) 100² over (50 + 500).
const FF: Face = face(100 * 100, 100, DT_F);
const CC_G: Face = face(1000 * 1000, 1000, DT_C);
const CFG: Face = face(100 * 100, 550, DT_F);

fn ci(x: usize, y: usize, z: usize) -> usize { (z * NC + y) * NC + x }
fn fi(x: usize, y: usize, z: usize) -> usize { (z * NF + y) * NF + x }
fn temp(e: i64, c: i64) -> i64 { e.div_euclid(c) }

/// The all-fine reference grid.
fn step_fine_grid(e: &mut [i64], d: &mut [i64]) {
    d.iter_mut().for_each(|v| *v = 0);
    for z in 0..NF { for y in 0..NF { for x in 0..NF {
        let i = fi(x, y, z);
        let t = temp(e[i], CF);
        for (dx, dy, dz) in [(1, 0, 0), (0, 1, 0), (0, 0, 1)] {
            let (a, b, c) = (x + dx, y + dy, z + dz);
            if a >= NF || b >= NF || c >= NF { continue }
            let j = fi(a, b, c);
            let f = flux(FF, t - temp(e[j], CF));
            d[i] -= f; d[j] += f;
        }
    }}}
    for (v, dv) in e.iter_mut().zip(d.iter()) { *v += dv }
}

struct Amr {
    coarse: Vec<i64>,
    /// Fine cells of each refined coarse cell (None: not refined).
    fine: Vec<Option<Vec<i64>>>,
    reflux: bool,
    /// Fine cell-steps and coarse cell-steps done (the cost).
    work: u64,
}

const SIDES: [(i32, i32, i32); 6] = [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)];

impl Amr {
    fn refined(&self, c: usize) -> bool { self.fine[c].is_some() }
    fn neighbour(c: (usize, usize, usize), s: (i32, i32, i32)) -> Option<(usize, usize, usize)> {
        let (x, y, z) = (c.0 as i32 + s.0, c.1 as i32 + s.1, c.2 as i32 + s.2);
        if x < 0 || y < 0 || z < 0 || x >= NC as i32 || y >= NC as i32 || z >= NC as i32 { return None }
        Some((x as usize, y as usize, z as usize))
    }
    fn total(&self) -> i128 {
        let mut s = 0i128;
        for c in 0..NC * NC * NC {
            match &self.fine[c] { Some(f) => s += f.iter().map(|&v| v as i128).sum::<i128>(), None => s += self.coarse[c] as i128 }
        }
        s
    }

    fn coarse_step(&mut self) -> Vec<i64> {
        let mut d = vec![0i64; NC * NC * NC];
        for z in 0..NC { for y in 0..NC { for x in 0..NC {
            let i = ci(x, y, z);
            let t = temp(self.coarse[i], CC);
            for s in [(1, 0, 0), (0, 1, 0), (0, 0, 1)] {
                let Some(n) = Self::neighbour((x, y, z), s) else { continue };
                let j = ci(n.0, n.1, n.2);
                if self.refined(i) && self.refined(j) { continue } // fine level owns it
                let f = flux(CC_G, t - temp(self.coarse[j], CC));
                d[i] -= f; d[j] += f;
            }
        }}}
        self.work += (NC * NC * NC) as u64;
        d
    }

    /// Fine substeps for one coarse step. `old`/`new` are the coarse cells'
    /// energies before and after the coarse step (for the time-interpolated
    /// ghost). Returns the flux register: per uncovered coarse cell, the sum
    /// of fine fluxes that went into it.
    fn fine_steps(&mut self, old: &[i64], new: &[i64]) -> Vec<i64> {
        let mut reg = vec![0i64; NC * NC * NC];
        let refined: Vec<usize> = (0..NC * NC * NC).filter(|&c| self.refined(c)).collect();
        for sub in 0..R as i64 {
            // Ghost temperature at the middle of this substep.
            let ghost = |c: usize| -> i64 {
                let (a, b) = (temp(old[c], CC), temp(new[c], CC));
                a + (b - a) * (2 * sub + 1) / (2 * R as i64)
            };
            let mut deltas: Vec<Vec<i64>> = refined.iter().map(|_| vec![0i64; R * R * R]).collect();
            let slot = |c: usize| refined.binary_search(&c).ok();
            for (bi, &c) in refined.iter().enumerate() {
                let cc = (c % NC, (c / NC) % NC, c / (NC * NC));
                let f = self.fine[c].as_ref().unwrap();
                let local = |x: usize, y: usize, z: usize| (z * R + y) * R + x;
                for z in 0..R { for y in 0..R { for x in 0..R {
                    let i = local(x, y, z);
                    let t = temp(f[i], CF);
                    // + faces inside the block, or into a refined neighbour block.
                    for (ax, s) in [(0, (1, 0, 0)), (1, (0, 1, 0)), (2, (0, 0, 1))] {
                        let p = [x, y, z];
                        if p[ax] + 1 < R {
                            let mut q = p; q[ax] += 1;
                            let j = local(q[0], q[1], q[2]);
                            let fl = flux(FF, t - temp(f[j], CF));
                            deltas[bi][i] -= fl; deltas[bi][j] += fl;
                            continue;
                        }
                        let Some(n) = Self::neighbour(cc, s) else { continue };
                        let nc = ci(n.0, n.1, n.2);
                        let Some(nb) = slot(nc) else { continue }; // coarse side: handled below
                        let mut q = p; q[ax] = 0;
                        let j = local(q[0], q[1], q[2]);
                        let tn = temp(self.fine[nc].as_ref().unwrap()[j], CF);
                        let fl = flux(FF, t - tn);
                        deltas[bi][i] -= fl; deltas[nb][j] += fl;
                    }
                }}}
                // Coarse–fine faces on all six sides.
                for s in SIDES {
                    let Some(n) = Self::neighbour(cc, s) else { continue };
                    let nc = ci(n.0, n.1, n.2);
                    if self.refined(nc) { continue }
                    let tg = ghost(nc);
                    let ax = if s.0 != 0 { 0 } else if s.1 != 0 { 1 } else { 2 };
                    let edge = if s.0 + s.1 + s.2 > 0 { R - 1 } else { 0 };
                    for u in 0..R { for v in 0..R {
                        let mut q = [0usize; 3];
                        let mut k = 0;
                        for a in 0..3 { if a == ax { q[a] = edge } else { q[a] = [u, v][k]; k += 1 } }
                        let i = local(q[0], q[1], q[2]);
                        let fl = flux(CFG, temp(f[i], CF) - tg);
                        deltas[bi][i] -= fl;
                        reg[nc] += fl;
                    }}
                }
            }
            for (bi, &c) in refined.iter().enumerate() {
                let f = self.fine[c].as_mut().unwrap();
                for (v, dv) in f.iter_mut().zip(&deltas[bi]) { *v += dv }
            }
            self.work += (refined.len() * R * R * R) as u64;
        }
        reg
    }

    fn step(&mut self) {
        let old = self.coarse.clone();
        let d = self.coarse_step();
        // What each uncovered cell was charged across coarse–fine faces.
        let mut charged = vec![0i64; NC * NC * NC];
        for z in 0..NC { for y in 0..NC { for x in 0..NC {
            let i = ci(x, y, z);
            if !self.refined(i) { continue }
            for s in SIDES {
                let Some(n) = Self::neighbour((x, y, z), s) else { continue };
                let j = ci(n.0, n.1, n.2);
                if self.refined(j) { continue }
                // flux from j into i, as coarse_step computed it
                charged[j] += flux(CC_G, temp(old[j], CC) - temp(old[i], CC));
            }
        }}}
        for (v, dv) in self.coarse.iter_mut().zip(&d) { *v += dv }
        let new = self.coarse.clone();
        let reg = self.fine_steps(&old, &new);
        if self.reflux {
            for c in 0..NC * NC * NC {
                if self.refined(c) { continue }
                // Give back what the coarse step charged it; apply what the fine faces gave it.
                self.coarse[c] += charged[c] + reg[c];
            }
        }
        // Restrict.
        for c in 0..NC * NC * NC {
            if let Some(f) = &self.fine[c] { self.coarse[c] = f.iter().sum() }
        }
    }

    /// Refine a coarse cell: split its energy into 1000 fine cells, with a
    /// limited linear profile from its coarse neighbours, summing exactly.
    fn refine(&mut self, c: usize) {
        if self.refined(c) { return }
        let cc = (c % NC, (c / NC) % NC, c / (NC * NC));
        let t0 = temp(self.coarse[c], CC);
        let mut slope = [0i64; 3]; // mK per coarse cell, minmod-limited
        for ax in 0..3 {
            let mut s = [(0i32, 0i32, 0i32); 2];
            let mut p = [0i32; 3]; p[ax] = 1; s[0] = (p[0], p[1], p[2]);
            s[1] = (-p[0], -p[1], -p[2]);
            let tp = Self::neighbour(cc, s[0]).map(|n| temp(self.coarse[ci(n.0, n.1, n.2)], CC));
            let tm = Self::neighbour(cc, s[1]).map(|n| temp(self.coarse[ci(n.0, n.1, n.2)], CC));
            let (a, b) = (tp.map(|t| t - t0).unwrap_or(0), tm.map(|t| t0 - t).unwrap_or(0));
            slope[ax] = if a.signum() == b.signum() { a.abs().min(b.abs()) * a.signum() } else { 0 };
        }
        let mut f = vec![0i64; R * R * R];
        let mut sum = 0i64;
        for z in 0..R { for y in 0..R { for x in 0..R {
            // offset from the coarse centre, in coarse cells: (2k+1-R)/(2R)
            let off = |k: usize| 2 * k as i64 + 1 - R as i64;
            let t = t0 + (slope[0] * off(x) + slope[1] * off(y) + slope[2] * off(z)) / (2 * R as i64);
            let e = t * CF;
            f[(z * R + y) * R + x] = e;
            sum += e;
        }}}
        // Exact sum: spread the difference one µJ at a time, in index order.
        let diff = self.coarse[c] - sum;
        let n = (R * R * R) as i64;
        let (q, r) = (diff.div_euclid(n), diff.rem_euclid(n));
        for (k, v) in f.iter_mut().enumerate() { *v += q + if (k as i64) < r { 1 } else { 0 } }
        debug_assert_eq!(f.iter().sum::<i64>(), self.coarse[c]);
        self.fine[c] = Some(f);
    }

    fn coarsen(&mut self, c: usize) {
        if let Some(f) = self.fine[c].take() { self.coarse[c] = f.iter().sum() }
    }

    fn set_refined(&mut self, want: &BTreeSet<usize>) {
        for c in 0..NC * NC * NC {
            if want.contains(&c) { self.refine(c) } else { self.coarsen(c) }
        }
    }

    /// Coarse-cell mean temperatures (mK).
    fn coarse_temps(&self) -> Vec<i64> {
        (0..NC * NC * NC).map(|c| match &self.fine[c] {
            Some(f) => temp(f.iter().sum(), CC),
            None => temp(self.coarse[c], CC),
        }).collect()
    }
    fn fine_temp_at(&self, x: usize, y: usize, z: usize) -> i64 {
        let c = ci(x / R, y / R, z / R);
        match &self.fine[c] {
            Some(f) => temp(f[((z % R) * R + y % R) * R + x % R], CF),
            None => temp(self.coarse[c], CC),
        }
    }
}

/// Which coarse cells to refine: any whose temperature spread (inside it,
/// if refined, or against its six neighbours, if not) exceeds `thresh` mK,
/// grown by one cell in every direction (a buffer, so a front can't outrun
/// the fine region between regrids).
fn tags(amr: &Amr, thresh: i64) -> BTreeSet<usize> {
    let t = amr.coarse_temps();
    let mut hot = Vec::new();
    for c in 0..NC * NC * NC {
        let cc = (c % NC, (c / NC) % NC, c / (NC * NC));
        let spread = match &amr.fine[c] {
            Some(f) => { let (lo, hi) = f.iter().fold((i64::MAX, i64::MIN), |(lo, hi), &e| (lo.min(temp(e, CF)), hi.max(temp(e, CF)))); hi - lo }
            None => SIDES.iter().filter_map(|&s| Amr::neighbour(cc, s)).map(|n| (t[c] - t[ci(n.0, n.1, n.2)]).abs()).max().unwrap_or(0),
        };
        if spread > thresh { hot.push(cc) }
    }
    let mut out = BTreeSet::new();
    for (x, y, z) in hot {
        for dz in -1i32..=1 { for dy in -1i32..=1 { for dx in -1i32..=1 {
            if let Some(n) = Amr::neighbour((x, y, z), (dx, dy, dz)) { out.insert(ci(n.0, n.1, n.2)); }
        }}}
    }
    out
}

fn initial_fine() -> Vec<i64> {
    // 290 K everywhere; a 40 cm cube at 1200 K centred at (4 m, 4 m, 4 m).
    let mut e = vec![AMBIENT * CF; NF * NF * NF];
    for z in 38..42 { for y in 38..42 { for x in 38..42 { e[fi(x, y, z)] = 1_200_000 * CF } } }
    e
}

fn main() {
    let steps: usize = std::env::var("STEPS").ok().and_then(|v| v.parse().ok()).unwrap_or(200);
    let thresh: i64 = std::env::var("THRESH_MK").ok().and_then(|v| v.parse().ok()).unwrap_or(2_000);
    let every: usize = std::env::var("REGRID").ok().and_then(|v| v.parse().ok()).unwrap_or(5);
    let init = initial_fine();
    let start: i128 = init.iter().map(|&v| v as i128).sum();

    // Reference: all fine.
    let t0 = std::time::Instant::now();
    let mut reference = init.clone();
    let mut d = vec![0i64; reference.len()];
    for _ in 0..steps * R { step_fine_grid(&mut reference, &mut d) }
    let ref_ms = t0.elapsed().as_millis();
    let ref_gap = reference.iter().map(|&v| v as i128).sum::<i128>() - start;
    let ref_coarse: Vec<i64> = (0..NC * NC * NC).map(|c| {
        let (cx, cy, cz) = (c % NC, (c / NC) % NC, c / (NC * NC));
        let mut s = 0i64;
        for z in 0..R { for y in 0..R { for x in 0..R { s += reference[fi(cx * R + x, cy * R + y, cz * R + z)] } } }
        temp(s, CC)
    }).collect();

    let mut rows = Vec::new();
    for (name, reflux, adaptive) in [("adaptive, refluxed", true, true), ("adaptive, not refluxed", false, true), ("all coarse", true, false)] {
        let mut coarse = vec![0i64; NC * NC * NC];
        for c in 0..NC * NC * NC {
            let (cx, cy, cz) = (c % NC, (c / NC) % NC, c / (NC * NC));
            for z in 0..R { for y in 0..R { for x in 0..R { coarse[c] += init[fi(cx * R + x, cy * R + y, cz * R + z)] } } }
        }
        let mut amr = Amr { coarse: coarse.clone(), fine: vec![None; NC * NC * NC], reflux, work: 0 };
        // Start refined where the initial state needs it, with the fine
        // initial state copied in (no prolongation needed).
        let mut probe_amr = Amr { coarse: coarse.clone(), fine: vec![None; NC * NC * NC], reflux, work: 0 };
        for c in 0..NC * NC * NC {
            let (cx, cy, cz) = (c % NC, (c / NC) % NC, c / (NC * NC));
            let mut f = vec![0i64; R * R * R];
            for z in 0..R { for y in 0..R { for x in 0..R { f[(z * R + y) * R + x] = init[fi(cx * R + x, cy * R + y, cz * R + z)] } } }
            probe_amr.fine[c] = Some(f);
        }
        let start_tags = tags(&probe_amr, thresh);
        if adaptive {
            for &c in &start_tags {
                let (cx, cy, cz) = (c % NC, (c / NC) % NC, c / (NC * NC));
                let mut f = vec![0i64; R * R * R];
                for z in 0..R { for y in 0..R { for x in 0..R { f[(z * R + y) * R + x] = init[fi(cx * R + x, cy * R + y, cz * R + z)] } } }
                amr.fine[c] = Some(f);
            }
        }
        let t0 = std::time::Instant::now();
        let mut worst_gap = 0i128;
        let (mut most, mut least, mut regrids) = (0usize, usize::MAX, 0usize);
        for s in 0..steps {
            if adaptive && s > 0 && s % every == 0 {
                let want = tags(&amr, thresh);
                let before: BTreeSet<usize> = (0..NC * NC * NC).filter(|&c| amr.refined(c)).collect();
                if want != before { regrids += 1 }
                amr.set_refined(&want);
            }
            let n = amr.fine.iter().filter(|f| f.is_some()).count();
            most = most.max(n); least = least.min(n);
            amr.step();
            let gap = amr.total() - start;
            if gap.abs() > worst_gap.abs() { worst_gap = gap }
        }
        let ms = t0.elapsed().as_millis();
        let gap = amr.total() - start;
        let ct = amr.coarse_temps();
        let err: Vec<i64> = ct.iter().zip(&ref_coarse).map(|(a, b)| (a - b).abs()).collect();
        let max_err = *err.iter().max().unwrap();
        let mean_err = err.iter().sum::<i64>() as f64 / err.len() as f64;
        // Peak temperature near the spot (fine cell at the original centre).
        let probe = amr.fine_temp_at(40, 40, 40);
        rows.push((name, gap, worst_gap, max_err, mean_err, probe, amr.work, ms, least, most, regrids));
    }

    println!("8 m cube of stone, 1 m coarse cells refined ×10 to 10 cm; {} coarse steps of {} s ({} fine steps of {} s), {:.1} days.",
        steps, DT_C / 1000, steps * R, DT_F / 1000, (steps as i64 * DT_C) as f64 / 86_400_000.0);
    println!("A 40 cm cube at 1200 K in 290 K stone. Refine where a coarse cell's spread > {} K (+1 cell buffer), regrid every {every} coarse steps.\n", thresh as f64 / 1000.0);
    println!("| run | energy gap at end (µJ) | worst gap in run (µJ) | coarse-cell temp error, max / mean (mK) | temp at the spot (K) | cell-steps | ms | refined coarse cells (least–most) | regrids that changed the set |");
    println!("|---|---|---|---|---|---|---|---|---|");
    let ref_probe = temp(reference[fi(40, 40, 40)], CF);
    println!("| all fine (reference) | {} | – | 0 / 0 | {:.1} | {} | {} | {} | – |", ref_gap, ref_probe as f64 / 1000.0, (NF * NF * NF * steps * R) as u64, ref_ms, NC * NC * NC);
    for (name, gap, worst, max_err, mean_err, probe, work, ms, least, most, regrids) in rows {
        println!("| {name} | {gap} | {worst} | {max_err} / {mean_err:.0} | {:.1} | {work} | {ms} | {least}–{most} | {regrids} |", probe as f64 / 1000.0);
    }
}
