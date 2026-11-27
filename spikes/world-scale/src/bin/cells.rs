//! Bytes per active cell: what fv-core uses today, and what a brick layout
//! for the GPU would use, measured on the real dead-tree scene.
//!
//!     cargo run --release --bin cells
//!
//! 1. Today: `size_of::<Cell>()`, and every byte the process holds (a
//!    counting allocator) after building the scene and at the peak of a
//!    tick, divided by the cell count.
//! 2. What the cells actually are in that scene: open air, dense solid
//!    (no gas space), porous solid (solid with gas), open water.
//! 3. Lossless compact layouts (the same integers, narrowed to the widths
//!    their ranges need), per 8³ brick, with each brick storing only the
//!    fields any of its cells uses.
use fv_core::rad::Sky;
use fv_core::scene::crib_and_tree;
use fv_core::sim::Sim;
use fv_core::cell::Cell;
use fv_core::world::B;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = System.alloc(l);
        let now = LIVE.fetch_add(l.size(), Relaxed) + l.size();
        PEAK.fetch_max(now, Relaxed);
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size(), Relaxed);
        System.dealloc(p, l)
    }
}
#[global_allocator]
static A: Counting = Counting;

/// The widest value each field reaches over the run, to check the narrow
/// widths below hold every value exactly.
#[derive(Default, Debug)]
struct Widest { solid: i64, water: i64, gas: i64, energy: i64, solid_energy: i64, skin: i64, nuclear: i64 }

fn bits(v: i64) -> u32 { 64 - (v.max(1) as u64).leading_zeros() }

fn main() {
    let ticks: u64 = std::env::var("TICKS").ok().and_then(|v| v.parse().ok()).unwrap_or(60);
    let base = LIVE.load(Relaxed);
    let (w, _torch) = crib_and_tree(4, 48, 48, true);
    let cells = w.cells.len();
    let mut sim = Sim::new(w, Sky::afternoon());
    let built = LIVE.load(Relaxed) - base;
    PEAK.store(LIVE.load(Relaxed), Relaxed);
    let mut widest = Widest::default();
    for _ in 0..ticks {
        sim.step();
        for c in &sim.world.cells {
            widest.solid = widest.solid.max(c.solid.iter().map(|m| m.0).max().unwrap());
            widest.water = widest.water.max(c.water.0);
            widest.gas = widest.gas.max(c.gas.iter().map(|m| m.0).max().unwrap());
            widest.energy = widest.energy.max(c.energy.0);
            widest.solid_energy = widest.solid_energy.max(c.solid_energy.0);
            widest.skin = widest.skin.max(c.skin.0);
            widest.nuclear = widest.nuclear.max(c.nuclear.0);
        }
    }
    let peak = PEAK.load(Relaxed) - base;
    let w = &sim.world;

    println!("## 1. fv-core today (dead tree, {}×{}×{} = {} cells, {} ticks)\n", w.nx, w.ny, w.nz, cells, ticks);
    println!("| | bytes | per cell |\n|---|---|---|");
    println!("| `size_of::<Cell>()` | {} | {} |", std::mem::size_of::<Cell>(), std::mem::size_of::<Cell>());
    println!("| held after building the scene | {} | {:.0} |", built, built as f64 / cells as f64);
    println!("| peak during ticks | {} | {:.0} |", peak, peak as f64 / cells as f64);

    // 2. What kinds of cell the scene holds, and 3. brick layouts.
    #[derive(Clone, Copy, PartialEq)]
    enum Kind { Air, Dense, Porous, Water }
    let kind = |c: &Cell| -> Kind {
        let solid = c.solid_mass().0 > 0;
        let gas = c.gas_mass().0 > 0;
        if !solid && c.water.0 > 0 { return Kind::Water }
        if !solid { return Kind::Air }
        if gas { return Kind::Porous }
        Kind::Dense
    };
    let mut n = [0usize; 4];
    for c in &w.cells { n[kind(c) as usize] += 1 }
    println!("\n## 2. What the cells are (end of run)\n");
    println!("| open air | dense solid | porous solid | open water |\n|---|---|---|---|");
    println!("| {} ({:.0} %) | {} ({:.0} %) | {} ({:.0} %) | {} |", n[0], pct(n[0], cells), n[1], pct(n[1], cells), n[2], pct(n[2], cells), n[3]);

    println!("\n## 3. Widest value of each store over the run (bits needed)\n");
    println!("| store | widest | bits |\n|---|---|---|");
    for (name, v) in [("solid part (µg)", widest.solid), ("water (µg)", widest.water), ("one gas (µg)", widest.gas),
        ("gas energy (µJ)", widest.energy), ("solid energy (µJ)", widest.solid_energy), ("skin (µJ)", widest.skin), ("nuclear (µJ)", widest.nuclear)] {
        println!("| {name} | {v} | {} |", bits(v));
    }

    // Compact, lossless field groups (bytes per cell). Masses stay whole µg in
    // u32 (to 4.29 kg; a cell of the densest substance is 2.7 kg); energies stay
    // whole µJ in 64 bits (two u32 words on the GPU: WGSL has no 64-bit ints).
    const HEAD: usize = 4; // substance, bonds + lit, front
    const GAS: usize = 7 * 4 + 8; // 7 gases + gas energy
    const SOLID: usize = 3 * 4 + 4 + 8 + 8; // 3 parts + water + solid energy + skin
    const NUCLEAR: usize = 8;
    let full = HEAD + GAS + SOLID + NUCLEAR;
    // Per brick: store a field group only if some cell in the brick needs it.
    let (bx, by, bz) = (w.nx / B, w.ny / B, w.nz / B);
    let mut typed = 0usize;
    let mut uniform = 0usize;
    let mut bricks_by = [0usize; 4]; // air-only, has solid, has gas+solid, uniform
    for k in 0..bz { for j in 0..by { for i in 0..bx {
        let mut has_gas = false; let mut has_solid = false; let mut has_nuc = false;
        let first = w.cells[w.idx(i * B, j * B, k * B)];
        let mut same = true;
        for z in 0..B { for y in 0..B { for x in 0..B {
            let c = &w.cells[w.idx(i * B + x, j * B + y, k * B + z)];
            has_gas |= c.gas_mass().0 > 0 || c.energy.0 > 0;
            has_solid |= c.has_solid();
            has_nuc |= c.nuclear.0 > 0;
            same &= *c == first;
        }}}
        let per = HEAD + if has_gas { GAS } else { 0 } + if has_solid { SOLID } else { 0 } + if has_nuc { NUCLEAR } else { 0 };
        if same { uniform += 1; typed += full; bricks_by[3] += 1; continue }
        typed += per * B * B * B;
        bricks_by[if !has_solid { 0 } else if has_gas { 2 } else { 1 }] += 1;
    }}}
    let nb = bx * by * bz;
    println!("\n## 4. Bricks ({B}³, {nb} of them)\n");
    println!("| air only | solid, no gas | solid and gas | every cell identical |\n|---|---|---|---|");
    println!("| {} | {} | {} | {} |", bricks_by[0], bricks_by[1], bricks_by[2], uniform);

    println!("\n## 5. Bytes per cell, by layout (state only, one copy)\n");
    println!("| layout | bytes per cell |\n|---|---|");
    println!("| fv-core `Cell` (array of structs, 64-bit everything) | {} |", std::mem::size_of::<Cell>());
    println!("| compact, every field (u32 masses, 64-bit energies) | {full} |");
    println!("| compact, per-brick field groups (this scene) | {:.1} |", typed as f64 / cells as f64);
    println!("\nDouble-buffered for read-previous/write-own: ×2. Brick index (hash or 2-level table, ~32 B per {}-cell brick): {:.2} B per cell.",
        B * B * B, 32.0 / (B * B * B) as f64);
    println!("Halo: kernels read neighbours through the index, so no copied ghost layer (a copied 1-cell halo on {B}³ would be ×{:.2}).",
        ((B + 2) * (B + 2) * (B + 2)) as f64 / (B * B * B) as f64);
}

fn pct(a: usize, b: usize) -> f64 { 100.0 * a as f64 / b as f64 }
