//! What holds things up: bonds between cells, the loads they carry, and
//! what happens when they can't.
//!
//! There is no "terrain" and no "structure" flag. A bonded face between two
//! solid cells is a spring with a strength; gravity pulls on every cell; only
//! the bottom of the world is fixed. A lattice solve (cells as nodes, bonds as
//! springs; Herrmann & Roux 1990) finds how much each bond is stretched,
//! squashed or sheared, and a bond loaded past what it can take breaks. Pieces
//! no longer joined to anything that's held up come loose and fall.
//!
//! Because a bond's strength depends on the cells' state, charring weakens
//! wood, and a post burnt half through carries its load on half the bonds,
//! bending as one side stretches and the other is squashed, until it gives.
//!
//! Solver shortcut (a speed measure, not a rule about materials): a cell
//! bonded on all six faces, inside a solid region joined to the bottom of the
//! world, is treated as rigid. At these loads bulk rock and soil strain by
//! nanometres; solving them would change nothing but the cost.
//!
//! Elastic energy stored in bonds is not in the books: a loaded post holds a
//! few millijoules, against megajoules of heat in a fire.

use crate::cell::{Cell, BOND_X, BOND_Y, BOND_Z};
use crate::substance::{Substance, NONE};
use crate::world::World;
use std::collections::BTreeMap;

const BITS: [u8; 3] = [BOND_X, BOND_Y, BOND_Z];
/// Gravity, m/s².
const G: f64 = 9.806_65;
/// Face area (m²) and bond length (m) of a 10 cm cell.
const AREA: f64 = 0.01;
const LENGTH: f64 = 0.1;
/// Shear stiffness as a share of axial (≈ G/E for most solids).
const SHEAR_STIFFNESS: f64 = 0.4;
/// What loose rubble bears in compression, kPa, and how stiff it is, MPa.
const GRANULAR_KPA: f64 = 30.0;
const GRANULAR_MPA: f64 = 20.0;
/// Shear strength as a share of tensile.
const SHEAR_STRENGTH: f64 = 0.6;

/// What a bond can take and how stiff it is.
#[derive(Copy, Clone, Debug)]
pub struct Bond {
    /// N/m.
    pub stiffness: f64,
    /// N, pulling apart / crushing / sliding.
    pub tension: f64,
    pub compression: f64,
    pub shear: f64,
}

/// A bond between two cells of these substances in this state. The weaker
/// side decides (a brick wall gives at its mortar), and charred or burnt-away
/// matter holds in proportion to what's left of it.
pub fn bond(a: &Cell, b: &Cell, subs: &[Substance]) -> Bond {
    let (sa, sb) = (&subs[a.substance as usize].strength, &subs[b.substance as usize].strength);
    let ia = a.integrity_permille(subs) as f64 / 1000.0;
    let ib = b.integrity_permille(subs) as f64 / 1000.0;
    let intact = ia.min(ib);
    let tensile = (sa.tensile_kpa.min(sb.tensile_kpa) as f64) * 1e3 * AREA * intact;
    let compressive = (sa.compressive_kpa.min(sb.compressive_kpa) as f64) * 1e3 * AREA * intact;
    // Two half-bonds in series.
    let (ea, eb) = (sa.stiffness_mpa as f64 * 1e6 * ia, sb.stiffness_mpa as f64 * 1e6 * ib);
    let e = if ea + eb > 0.0 { 2.0 * ea * eb / (ea + eb) } else { 0.0 };
    Bond { stiffness: (e * AREA / LENGTH).max(1.0), tension: tensile, compression: compressive, shear: tensile * SHEAR_STRENGTH }
}

/// Whether the face between (x, y, z) and its + neighbour along `axis` is bonded.
#[inline]
fn bonded(w: &World, x: usize, y: usize, z: usize, axis: usize) -> bool {
    w.get(x, y, z).bonds & BITS[axis] != 0
}

/// A cell's bonded neighbours: (neighbour index in the grid, axis, +1/−1).
fn bonded_neighbours(w: &World, x: usize, y: usize, z: usize, out: &mut Vec<(usize, usize, i64, [usize; 3])>) {
    out.clear();
    for axis in 0..3 {
        let p = [x, y, z];
        if p[axis] + 1 < [w.nx, w.ny, w.nz][axis] && bonded(w, x, y, z, axis) {
            let mut q = p;
            q[axis] += 1;
            out.push((w.idx(q[0], q[1], q[2]), axis, 1, q));
        }
        if p[axis] > 0 {
            let mut q = p;
            q[axis] -= 1;
            if bonded(w, q[0], q[1], q[2], axis) { out.push((w.idx(q[0], q[1], q[2]), axis, -1, q)) }
        }
    }
}

/// Results of one look at the structure.
#[derive(Debug, Default)]
pub struct Report {
    /// Bonds that broke: (cell, axis) of the lower cell of the face.
    pub broke: Vec<([usize; 3], usize)>,
    /// Groups of cells no longer held up by anything (each will fall as one piece).
    pub loose: Vec<Vec<[usize; 3]>>,
    /// The highest load on any bond, as a share of what it can take.
    pub worst: f64,
    /// Cells crushed by what rests on them: they no longer hold anything up.
    pub crushed: Vec<[usize; 3]>,
}

/// Find each bond's load, break what's overloaded, and gather what's loose.
pub fn settle(w: &mut World) -> Report {
    let (nx, ny, nz) = (w.nx, w.ny, w.nz);
    let n = nx * ny * nz;
    let at = |p: [usize; 3]| (p[2] * ny + p[1]) * nx + p[0]; // dense index for scratch
    // 1. Which solid cells are rigid bulk joined to the bottom of the world.
    let mut bulk = vec![false; n];
    let mut nb = Vec::with_capacity(6);
    for z in 0..nz { for y in 0..ny { for x in 0..nx {
        if w.get(x, y, z).substance == NONE { continue }
        bonded_neighbours(w, x, y, z, &mut nb);
        let all_faces = nb.len() == 6 - [x == 0 || x + 1 == nx, y == 0 || y + 1 == ny, z == 0 || z + 1 == nz].iter().filter(|e| **e).count();
        bulk[at([x, y, z])] = all_faces && nb.len() >= 4;
    }}}
    let mut grounded = vec![false; n];
    let mut stack: Vec<[usize; 3]> = Vec::new();
    for y in 0..ny { for x in 0..nx { if bulk[at([x, y, 0])] { grounded[at([x, y, 0])] = true; stack.push([x, y, 0]) } } }
    while let Some([x, y, z]) = stack.pop() {
        bonded_neighbours(w, x, y, z, &mut nb);
        for &(_, _, _, q) in nb.iter() {
            let d = at(q);
            if bulk[d] && !grounded[d] { grounded[d] = true; stack.push(q) }
        }
    }
    // 2. Stacks to solve: solid cells joined by bonds, or by resting on one
    // another (a block on a post, a post on rock). Each stack is solved as one
    // lattice so a load sitting on top weighs on everything under it.
    let solid = |x: usize, y: usize, z: usize| w.get(x, y, z).substance != NONE && w.get(x, y, z).solid_mass().0 > 0;
    let rests_on = |x: usize, y: usize, z: usize| z > 0 && solid(x, y, z - 1) && !bonded(w, x, y, z - 1, 2);
    let mut stack_of = vec![usize::MAX; n];
    let mut stacks: Vec<Vec<[usize; 3]>> = Vec::new();
    for z in 0..nz { for y in 0..ny { for x in 0..nx {
        let d = at([x, y, z]);
        if grounded[d] || stack_of[d] != usize::MAX || !solid(x, y, z) { continue }
        bonded_neighbours(w, x, y, z, &mut nb);
        let touches = rests_on(x, y, z) || (z + 1 < nz && solid(x, y, z + 1) && !bonded(w, x, y, z, 2));
        if nb.is_empty() && !touches && z > 0 { continue } // loose matter on its own (grass, ash)
        let id = stacks.len();
        let mut cells = Vec::new();
        stack_of[d] = id;
        stack.push([x, y, z]);
        while let Some(p) = stack.pop() {
            cells.push(p);
            bonded_neighbours(w, p[0], p[1], p[2], &mut nb);
            let mut next: Vec<[usize; 3]> = nb.iter().map(|t| t.3).collect();
            if rests_on(p[0], p[1], p[2]) { next.push([p[0], p[1], p[2] - 1]) }
            if p[2] + 1 < nz && solid(p[0], p[1], p[2] + 1) && !bonded(w, p[0], p[1], p[2], 2) { next.push([p[0], p[1], p[2] + 1]) }
            for q in next {
                let e = at(q);
                let free = q[2] > 0 || !grounded[e];
                if free && !grounded[e] && stack_of[e] == usize::MAX && (solid(q[0], q[1], q[2])) {
                    bonded_neighbours(w, q[0], q[1], q[2], &mut Vec::new());
                    stack_of[e] = id;
                    stack.push(q);
                }
            }
        }
        cells.sort_by_key(|p| at(*p));
        stacks.push(cells);
    }}}
    let mut report = Report::default();
    for cells in &stacks { solve_piece(w, cells, &grounded, &mut report) }
    let broke = report.broke.clone();
    let _ = (solid, rests_on);
    for (p, axis) in &broke {
        let c = w.get_mut(p[0], p[1], p[2]);
        c.bonds &= !BITS[*axis];
    }
    let solid = |x: usize, y: usize, z: usize| w.get(x, y, z).substance != NONE && w.get(x, y, z).solid_mass().0 > 0;
    // 3. After breaking: which bonded pieces are still held up, by a bond to
    // something held or by resting on something held? Work upward from the
    // bottom of the world until nothing changes.
    let mut held = grounded.clone();
    let mut piece_of = vec![usize::MAX; n];
    let mut pieces: Vec<Vec<[usize; 3]>> = Vec::new();
    for cells in &stacks { for p in cells {
        let d = at(*p);
        if piece_of[d] != usize::MAX { continue }
        let id = pieces.len();
        let mut group = Vec::new();
        piece_of[d] = id;
        stack.push(*p);
        while let Some(q) = stack.pop() {
            group.push(q);
            bonded_neighbours(w, q[0], q[1], q[2], &mut nb);
            for &(_, _, _, r) in nb.iter() {
                let e = at(r);
                if !grounded[e] && piece_of[e] == usize::MAX { piece_of[e] = id; stack.push(r) }
            }
        }
        group.sort_by_key(|q| at(*q));
        pieces.push(group);
    }}
    let mut crushed = vec![false; n];
    for p in &report.crushed { crushed[at(*p)] = true }
    let mut changed = true;
    let mut is_held = vec![false; pieces.len()];
    while changed {
        changed = false;
        for (id, group) in pieces.iter().enumerate() {
            if is_held[id] { continue }
            // Held by a bond to something held (or the world's bottom)...
            let bonded_hold = group.iter().any(|p| {
                if p[2] == 0 { return true }
                let mut v = Vec::new();
                bonded_neighbours(w, p[0], p[1], p[2], &mut v);
                v.iter().any(|t| held[at(t.3)])
            });
            // ...or resting on held, uncrushed cells with its centre of mass
            // over them (otherwise it tips over).
            let rests: Vec<[usize; 3]> = group.iter().copied().filter(|p| {
                // A crushed cell can't hold anything up, but it can lie on
                // the ground as rubble.
                p[2] > 0 && solid(p[0], p[1], p[2] - 1) && held[at([p[0], p[1], p[2] - 1])]
                    && !crushed[at([p[0], p[1], p[2] - 1])]
            }).collect();
            let balanced = !rests.is_empty() && {
                let (mut m, mut mx, mut my) = (0f64, 0f64, 0f64);
                for p in group {
                    let c = w.get(p[0], p[1], p[2]);
                    let cm = (c.solid_mass().0 + c.water.0) as f64;
                    m += cm;
                    mx += cm * (p[0] as f64 + 0.5);
                    my += cm * (p[1] as f64 + 0.5);
                }
                let (cx, cy) = (mx / m.max(1.0), my / m.max(1.0));
                let (x0, x1) = (rests.iter().map(|p| p[0]).min().unwrap() as f64, rests.iter().map(|p| p[0]).max().unwrap() as f64 + 1.0);
                let (y0, y1) = (rests.iter().map(|p| p[1]).min().unwrap() as f64, rests.iter().map(|p| p[1]).max().unwrap() as f64 + 1.0);
                cx >= x0 && cx <= x1 && cy >= y0 && cy <= y1
            };
            let ok = bonded_hold || balanced;
            if ok {
                is_held[id] = true;
                for p in group { held[at(*p)] = true }
                changed = true;
            }
        }
    }
    if std::env::var("FV_DEBUG_LOOSE").is_ok() {
        for (id, group) in pieces.iter().enumerate() {
            if is_held[id] { continue }
            for p in group.iter().take(3) {
                let below = if p[2] > 0 { Some([p[0], p[1], p[2] - 1]) } else { None };
                eprintln!("loose cell {:?} sub {} | below {:?}: solid {} held {} crushed {}", p, w.get(p[0], p[1], p[2]).substance,
                    below, below.map_or(false, |b| solid(b[0], b[1], b[2])), below.map_or(false, |b| held[at(b)]), below.map_or(false, |b| crushed[at(b)]));
            }
        }
    }
    for (id, group) in pieces.into_iter().enumerate() { if !is_held[id] { report.loose.push(group) } }
    report
}

/// Lattice statics for one held piece: displacements under gravity, then
/// each bond's force against its strength.
fn solve_piece(w: &World, cells: &[[usize; 3]], grounded: &[bool], report: &mut Report) {
    let (nx, ny) = (w.nx, w.ny);
    let at = |p: [usize; 3]| (p[2] * ny + p[1]) * nx + p[0];
    let index: BTreeMap<usize, usize> = cells.iter().enumerate().map(|(i, p)| (at(*p), i)).collect();
    // Bonds: (a, b or None if b is fixed, axis, sign from a to b, properties).
    // Links: (other cell or None if it's fixed, axis, properties, contact?).
    // A contact (resting, unbonded) only pushes: it carries compression along
    // the vertical and nothing else.
    let mut links: Vec<Vec<(Option<usize>, usize, Bond, bool)>> = vec![Vec::new(); cells.len()];
    let mut faces: Vec<(usize, Option<usize>, usize, [usize; 3], Bond)> = Vec::new();
    // Resting contacts: (upper cell, lower cell if it's in the piece, lower cell's place, properties).
    let mut contacts: Vec<(usize, Option<usize>, [usize; 3], Bond)> = Vec::new();
    let mut nb = Vec::with_capacity(6);
    for (i, p) in cells.iter().enumerate() {
        bonded_neighbours(w, p[0], p[1], p[2], &mut nb);
        for &(_, axis, dir, q) in nb.iter() {
            let other = index.get(&at(q)).copied();
            let fixed = grounded[at(q)] || q[2] == 0 && other.is_none();
            if other.is_none() && !fixed { continue }
            let b = bond(w.get(p[0], p[1], p[2]), w.get(q[0], q[1], q[2]), &w.subs);
            links[i].push((other, axis, b, false));
            // Record each face once: from its lower cell.
            let lower = if dir > 0 { *p } else { q };
            if dir > 0 || other.is_none() { faces.push((i, other, axis, lower, b)) }
        }
        // Resting on the bottom of the world: a fixed support.
        if p[2] == 0 {
            let mut b = bond(w.get(p[0], p[1], p[2]), w.get(p[0], p[1], p[2]), &w.subs);
            b.tension = f64::INFINITY;
            b.shear = f64::INFINITY;
            b.compression = f64::INFINITY;
            b.stiffness = b.stiffness.max(GRANULAR_MPA * 1e6 * AREA / LENGTH);
            links[i].push((None, 2, b, true));
        }
        // Resting on what's below, and being rested on from above.
        for dz in [-1i64, 1] {
            let qz = p[2] as i64 + dz;
            if qz < 0 || qz as usize >= w.nz { continue }
            let q = [p[0], p[1], qz as usize];
            let lower = if dz < 0 { q } else { *p };
            let (a, c) = (w.get(p[0], p[1], p[2]), w.get(q[0], q[1], q[2]));
            if c.substance == NONE || c.solid_mass().0 == 0 || w.get(lower[0], lower[1], lower[2]).bonds & BITS[2] != 0 { continue }
            let other = index.get(&at(q)).copied();
            if other.is_none() && !grounded[at(q)] { continue }
            let mut b = bond(a, c, &w.subs);
            // Contact can't pull or shear: only its crushing strength counts.
            // Even matter with no strength left bears some weight as a loose
            // pile (ash and char rubble, ~30 kPa), like any granular heap.
            b.tension = f64::INFINITY;
            b.shear = f64::INFINITY;
            b.compression = b.compression.max(GRANULAR_KPA * 1e3 * AREA);
            b.stiffness = b.stiffness.max(GRANULAR_MPA * 1e6 * AREA / LENGTH);
            if dz < 0 { contacts.push((i, other, q, b)) }
            links[i].push((other, 2, b, true));
        }
    }
    // Matter that's burnt away holds nothing: its bonds are gone and it
    // can't bear anything resting on it, whatever the loads.
    for &(_, _, axis, lower, b) in &faces {
        if b.tension < 1.0 || b.compression < 1.0 { report.broke.push((lower, axis)) }
    }
    for &(i, _, below, b) in &contacts {
        if b.compression < 1.0 {
            let weak = |p: [usize; 3]| w.get(p[0], p[1], p[2]).integrity_permille(&w.subs);
            report.crushed.push(if weak(cells[i]) <= weak(below) { cells[i] } else { below });
        }
    }
    // Gravity on each cell, N (downward).
    let weight: Vec<f64> = cells.iter().map(|p| {
        let c = w.get(p[0], p[1], p[2]);
        (c.solid_mass().0 + c.water.0) as f64 * 1e-9 * G
    }).collect();
    // Gravity only pulls down, so only the vertical displacement is loaded:
    // vertical bonds act as axial springs, horizontal ones carry vertical
    // shear, and bending shows up as different forces on either side of a
    // section. That's one symmetric, positive-definite system, solved by
    // conjugate gradients in a fixed order (deterministic). Contacts that
    // end up pulling are let go and the system solved again.
    let n = cells.len();
    let mut active: Vec<Vec<bool>> = links.iter().map(|l| vec![true; l.len()]).collect();
    let mut uz = vec![0.0f64; n];
    for _ in 0..4 {
        let k_of = |i: usize, li: usize| -> f64 {
            let (_, axis, b, _) = links[i][li];
            if axis == 2 { b.stiffness } else { b.stiffness * SHEAR_STIFFNESS }
        };
        // The active links as flat rows (compressed sparse rows): each row's
        // links in their order, the other cell's index (`n` for a fixed
        // support, whose displacement is the 0.0 kept at the end of the
        // vector) and the stiffness. The same products and sums in the same
        // order as walking `links`, without the branches and pointer-chasing.
        let mut row = Vec::with_capacity(n + 1);
        let mut col: Vec<u32> = Vec::new();
        let mut stiff: Vec<f64> = Vec::new();
        for i in 0..n {
            row.push(col.len() as u32);
            for (li, &(other, _, _, _)) in links[i].iter().enumerate() {
                if !active[i][li] { continue }
                col.push(other.map_or(n, |j| j) as u32);
                stiff.push(k_of(i, li));
            }
        }
        row.push(col.len() as u32);
        // `x` has n + 1 entries, the last 0.0 (the fixed supports).
        let apply = |x: &[f64], out: &mut [f64]| {
            for i in 0..n {
                let mut acc = 0.0;
                let xi = x[i];
                for l in row[i] as usize..row[i + 1] as usize {
                    acc += stiff[l] * (xi - x[col[l] as usize]);
                }
                out[i] = acc;
            }
        };
        // The dot products below are summed in index order from the same
        // start as `Iterator::sum` (so the rounding is the same), but inside
        // the loops that make their terms: the sum is a chain of dependent
        // additions, and the loop's other work fills the time between them.
        let zero: f64 = core::iter::empty::<f64>().sum();
        let bvec: Vec<f64> = weight.iter().map(|w| -w).collect();
        let mut x = uz.clone();
        let mut ax = vec![0.0; n];
        x.push(0.0);
        apply(&x, &mut ax);
        x.pop();
        let mut r: Vec<f64> = (0..n).map(|i| bvec[i] - ax[i]).collect();
        // Cells with no active links can't be solved for (they float); leave them.
        let mut p = r.clone();
        p.push(0.0);
        let mut rr: f64 = r.iter().map(|v| v * v).sum();
        let bb: f64 = bvec.iter().map(|v| v * v).sum::<f64>().max(1e-30);
        let mut ap = vec![0.0; n];
        for _ in 0..(4 * n + 200) {
            if rr <= bb * 1e-20 { break }
            // ap = A·p and pap = p·ap, together.
            let mut pap = zero;
            for i in 0..n {
                let mut acc = 0.0;
                let pi = p[i];
                for l in row[i] as usize..row[i + 1] as usize {
                    acc += stiff[l] * (pi - p[col[l] as usize]);
                }
                ap[i] = acc;
                pap += pi * acc;
            }
            if pap <= 0.0 { break }
            let alpha = rr / pap;
            let mut rr2 = zero;
            for i in 0..n { x[i] += alpha * p[i]; r[i] -= alpha * ap[i]; rr2 += r[i] * r[i] }
            let beta = rr2 / rr;
            rr = rr2;
            for i in 0..n { p[i] = r[i] + beta * p[i] }
        }
        uz = x;
        // Let go of contacts that are pulling (the cell above lifted off).
        let mut changed = false;
        for i in 0..n {
            for (li, &(other, _, _, contact)) in links[i].iter().enumerate() {
                if !contact { continue }
                let above = other.map_or(true, |j| cells[j][2] < cells[i][2]);
                let uo = other.map_or(0.0, |j| uz[j]);
                let gap = if above { uz[i] - uo } else { uo - uz[i] };
                let pressed = gap <= 0.0;
                if active[i][li] != pressed { active[i][li] = pressed; changed = true }
            }
        }
        if !changed { break }
    }
    let u: Vec<[f64; 3]> = uz.iter().map(|z| [0.0, 0.0, *z]).collect();
    // A piece with nothing holding part of it up is a mechanism, not a
    // structure: its "displacement" runs away. Don't break bonds on the
    // strength of that; whether it falls is decided by what holds it.
    let sagging = uz.iter().fold(0.0f64, |m, z| m.max(z.abs()));
    if !(sagging < 0.01) { return }
    if std::env::var("FV_DEBUG_STRUCT").is_ok() {
        // Force balance check: the axial force across each horizontal layer of
        // bonds should equal the weight above it.
        let mut by_z: BTreeMap<usize, f64> = BTreeMap::new();
        for &(i, other, axis, lower, b) in &faces {
            if axis != 2 { continue }
            let uj = other.map_or([0.0; 3], |j| u[j]);
            let (lo, hi) = if cells[i] == lower { (u[i], uj) } else { (uj, u[i]) };
            *by_z.entry(lower[2]).or_default() += b.stiffness * (hi[2] - lo[2]);
        }
        let total: f64 = weight.iter().sum();
        eprintln!("piece of {} cells, weight {:.0} N; vertical bond force by layer: {:?}", cells.len(), total, by_z.iter().map(|(z, f)| (*z, f.round())).collect::<Vec<_>>());
    }
    // A resting contact pressing harder than the lower cell can bear crushes
    // it (a burnt-out stump under a post).
    for &(i, other, below, b) in &contacts {
        let squeeze = b.stiffness * (other.map_or(0.0, |j| u[j][2]) - u[i][2]);
        if squeeze <= b.compression { continue }
        // The weaker of the two gives.
        let (up, down) = (cells[i], below);
        let strength = |p: [usize; 3]| {
            let c = w.get(p[0], p[1], p[2]);
            w.subs[c.substance as usize].strength.compressive_kpa * c.integrity_permille(&w.subs)
        };
        report.crushed.push(if strength(up) <= strength(down) { up } else { down });
    }
    // Forces in each bond against its strength.
    for &(i, other, axis, lower, b) in &faces {
        let ui = u[i];
        let uj = other.map_or([0.0; 3], |j| u[j]);
        // Stretch along the axis, measured from the lower cell to the upper.
        let i_is_lower = cells[i] == lower;
        let (lo, hi) = if i_is_lower { (ui, uj) } else { (uj, ui) };
        let stretch = hi[axis] - lo[axis];
        let axial = b.stiffness * stretch; // + pulls apart, − squashes
        let slide = ((0..3).filter(|d| *d != axis).map(|d| (hi[d] - lo[d]).powi(2)).sum::<f64>()).sqrt() * b.stiffness * SHEAR_STIFFNESS;
        let load = (if axial > 0.0 { axial / b.tension.max(1e-9) } else { -axial / b.compression.max(1e-9) }).max(slide / b.shear.max(1e-9));
        report.worst = report.worst.max(load);
        if load > 1.0 { report.broke.push((lower, axis)) }
    }
}
