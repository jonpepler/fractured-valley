//! Scenes for tests and recordings. Everything here only *places*
//! material; from the first tick on, physics does the rest.

use crate::cell::{Cell, BOND_X, BOND_Y, BOND_Z};
use crate::rng::hash;
use crate::substance::*;
use crate::units::*;
use crate::world::World;

/// Places a cell, ignoring anything outside the box (so scenes clip at any size).
fn put(w: &mut World, x: usize, y: usize, z: usize, c: Cell) {
    if x < w.nx && y < w.ny && z < w.nz { w.set(x, y, z, c) }
}

/// Smooth value noise in 0..1000 over cells, `scale` cells per feature.
pub fn noise(seed: u64, x: i64, y: i64, scale: i64) -> i64 {
    let (gx, gy) = (x.div_euclid(scale), y.div_euclid(scale));
    let (fx, fy) = (x.rem_euclid(scale) * 1000 / scale, y.rem_euclid(scale) * 1000 / scale);
    let v = |a: i64, b: i64| (hash(seed, a as u64, b as u64, 0x7e1) % 1000) as i64;
    let s = |t: i64| t * t * (3000 - 2 * t) / 1_000_000; // smoothstep, 0..1000
    let (sx, sy) = (s(fx), s(fy));
    let top = v(gx, gy) * (1000 - sx) + v(gx + 1, gy) * sx;
    let bot = v(gx, gy + 1) * (1000 - sx) + v(gx + 1, gy + 1) * sx;
    (top * (1000 - sy) + bot * sy) / 1_000_000
}

/// Bond every pair of neighbouring cells that are both solid (not air) and
/// for which `bonded(a, b)` says so. Natural rock and soil bond to each other;
/// each built thing decides its own.
pub fn bond_all(w: &mut World, bonded: impl Fn(u8, u8) -> bool) {
    for z in 0..w.nz { for y in 0..w.ny { for x in 0..w.nx {
        let a = w.get(x, y, z).substance;
        if a == NONE { continue }
        let mut bits = 0u8;
        for (axis, bit) in [(0usize, BOND_X), (1, BOND_Y), (2, BOND_Z)] {
            let mut p = [x, y, z];
            p[axis] += 1;
            if p[0] >= w.nx || p[1] >= w.ny || p[2] >= w.nz { continue }
            let b = w.get(p[0], p[1], p[2]).substance;
            if b != NONE && bonded(a, b) { bits |= bit }
        }
        w.get_mut(x, y, z).bonds = bits;
    }}}
}

/// Natural ground holds together; plants don't bond to anything structurally
/// except wood to wood and leaves to wood; masonry bonds through mortar.
pub fn natural_bonds(a: u8, b: u8) -> bool {
    let ground = |s: u8| s == STONE || s == SOIL || s == ORE;
    let wood = |s: u8| s == WOOD;
    (ground(a) && ground(b))
        || (wood(a) && wood(b))
        || (wood(a) && b == LEAVES) || (a == LEAVES && wood(b))
        // A 10 cm cell of foliage holds its twigs: the crown hangs together.
        || (a == LEAVES && b == LEAVES)
        || ((a == BRICK || a == MORTAR) && (b == BRICK || b == MORTAR) && (a == MORTAR || b == MORTAR || a == b))
        || (a == IRON && b == IRON)
}

pub struct Valley {
    pub world: World,
    /// Ground height (top solid cell) per column, x-fastest.
    pub ground: Vec<usize>,
    pub post: (usize, usize),
    pub tree: (usize, usize),
    pub crib: (usize, usize),
    pub ore: (usize, usize),
    pub wall: (usize, usize),
}

impl Valley {
    pub fn ground_at(&self, x: usize, y: usize) -> usize { self.ground[y * self.world.nx + x] }
}

/// A 9.6 m patch of the valley: grassy rolling ground over soil and rock, a
/// stone outcrop, a brick-and-mortar wall, a tree, a dry post carrying an iron
/// block, a crib of dry logs, and a lump of radioactive ore.
pub fn valley(seed: u64, n: usize, nz: usize, grass_moisture: i64) -> Valley {
    let mut w = World::new(n, n, nz, seed, celsius(18));
    let subs = w.subs.clone();
    let t = celsius(18);
    let stone = Cell::solid_of(STONE, 1000, 2, t, &subs);
    let soil = Cell::solid_of(SOIL, 1000, 150, t, &subs);
    let grass = Cell::solid_of(GRASS, 1000, grass_moisture, t, &subs);
    let mut ground = vec![0usize; n * n];
    for y in 0..n { for x in 0..n {
        let h = (10 + noise(seed, x as i64, y as i64, 24) * 6 / 1000 + noise(seed ^ 1, x as i64, y as i64, 9) * 2 / 1000) as usize;
        let rock_top = h - 3 - (noise(seed ^ 2, x as i64, y as i64, 12) * 3 / 1000) as usize;
        let outcrop = noise(seed ^ 3, x as i64, y as i64, 14) > 780;
        let top = if outcrop { h + 2 } else { h };
        for z in 0..=top {
            let c = if z <= rock_top || (outcrop && z > h - 2) { stone } else { soil };
            put(&mut w, x, y, z, c);
        }
        if !outcrop { put(&mut w, x, y, top + 1, grass) }
        ground[y * n + x] = top;
    }}
    let g = |x: usize, y: usize| ground[y * n + x];
    // A tree: trunk, a few branches, a crown of leaves (living: wet).
    let tree = (n * 3 / 10, n * 6 / 10);
    let living_wood = Cell::solid_of(WOOD, 1000, 900, t, &subs);
    let leaves = Cell::solid_of(LEAVES, 1000, 1400, t, &subs);
    let tb = g(tree.0, tree.1);
    let trunk_h = 22;
    for z in tb + 1..tb + trunk_h { for dy in 0..2 { for dx in 0..2 { put(&mut w, tree.0 + dx, tree.1 + dy, z, living_wood) } } }
    let crown_c = (tree.0 as i64, tree.1 as i64, (tb + trunk_h + 3) as i64);
    for z in (tb + 12)..(tb + trunk_h + 10).min(nz) { for y in tree.1.saturating_sub(9)..(tree.1 + 10).min(n) { for x in tree.0.saturating_sub(9)..(tree.0 + 10).min(n) {
        let (dx, dy, dz) = (x as i64 - crown_c.0, y as i64 - crown_c.1, (z as i64 - crown_c.2) * 3 / 2);
        let r2 = dx * dx + dy * dy + dz * dz;
        let wobble = noise(seed ^ 9, x as i64 * 3 + z as i64, y as i64 * 3, 4) * 30 / 1000;
        if r2 < 56 + wobble && z < nz && w.get(x, y, z).substance == NONE && hash(seed, x as u64, y as u64, z as u64) % 5 != 0 { put(&mut w, x, y, z, leaves) }
    }}}
    for (bx, by) in [(1i64, 0i64), (-1, 1), (0, -1), (1, 1)] {
        for k in 1..6i64 {
            let (x, y, z) = (crown_c.0 + bx * k, crown_c.1 + by * k, crown_c.2 - 4 + k / 2);
            if x >= 0 && y >= 0 && z >= 0 { put(&mut w, x as usize, y as usize, z as usize, living_wood) }
        }
    }
    // A dry post (dead wood, 12 % moisture) carrying an iron block.
    let post = (n * 6 / 10, n * 6 / 10);
    let dry_wood = Cell::solid_of(WOOD, 1000, 120, t, &subs);
    let pb = g(post.0, post.1);
    for z in pb + 1..pb + 10 { for dy in 0..2 { for dx in 0..2 { put(&mut w, post.0 + dx, post.1 + dy, z, dry_wood) } } }
    let iron = Cell::solid_of(IRON, 1000, 0, t, &subs);
    for z in pb + 10..pb + 13 { for dy in 0..3 { for dx in 0..3 { put(&mut w, post.0 + dx - 1 + 1, post.1 + dy - 1 + 1, z, iron) } } }
    // A crib of dry logs (2×2 logs, gaps of 2, alternating layers).
    let crib = (n * 7 / 10, n * 3 / 10);
    let cb = (0..10).flat_map(|dy| (0..10).map(move |dx| (dx, dy))).filter(|(dx, dy)| crib.0 + dx < n && crib.1 + dy < n).map(|(dx, dy)| g(crib.0 + dx, crib.1 + dy)).max().unwrap_or(0);
    for layer in 0..5 {
        let z = cb + 1 + layer * 2;
        for k in 0..3 { for a in 0..10 { for t2 in 0..2 { for dz in 0..2 {
            let (x, y) = if layer % 2 == 0 { (crib.0 + a, crib.1 + k * 4 + t2) } else { (crib.0 + k * 4 + t2, crib.1 + a) };
            put(&mut w, x, y, z + dz, dry_wood);
        }}}}
    }
    // A brick wall with mortar joints (bricks 3×1×1 cells... laid in courses).
    let wall = (n / 10, n * 2 / 10);
    let brick = Cell::solid_of(BRICK, 1000, 10, t, &subs);
    let mortar = Cell::solid_of(MORTAR, 1000, 20, t, &subs);
    let wb = (0..24).filter(|dx| wall.0 + dx < n).map(|dx| g(wall.0 + dx, wall.1)).max().unwrap_or(0);
    for course in 0..8 {
        let z = wb + 1 + course * 2;
        for dx in 0..24 { for dy in 0..2 {
            let joint = (dx + course % 2 * 2) % 4 == 3;
            put(&mut w, wall.0 + dx, wall.1 + dy, z, if joint { mortar } else { brick });
            put(&mut w, wall.0 + dx, wall.1 + dy, z + 1, mortar);
        }}
    }
    // Radioactive ore: a lump half buried.
    let ore = (n * 8 / 10, n * 8 / 10);
    let ore_cell = Cell::solid_of(ORE, 1000, 0, t, &subs);
    let ob = g(ore.0, ore.1);
    for z in ob - 1..ob + 2 { for y in ore.1..ore.1 + 3 { for x in ore.0..ore.0 + 3 { put(&mut w, x, y, z, ore_cell) } } }
    bond_all(&mut w, natural_bonds);
    Valley { world: w, ground, post, tree, crib, ore, wall }
}

/// A crib of dry logs over a bed of kindling, at the foot of a tree, on
/// bonded rock and soil. `dead` dries the tree out (a drought-killed tree).
/// Returns the world and where to hold a torch (kindling cells).
pub fn crib_and_tree(seed: u64, n: usize, nz: usize, dead: bool) -> (World, Vec<[usize; 3]>) {
    let mut w = World::new(n, n, nz, seed, celsius(18));
    let subs = w.subs.clone();
    let t = celsius(18);
    let stone = Cell::solid_of(STONE, 1000, 2, t, &subs);
    let soil = Cell::solid_of(SOIL, 1000, 150, t, &subs);
    let grass = Cell::solid_of(GRASS, 1000, 600, t, &subs);
    let g = 6;
    for y in 0..n { for x in 0..n {
        for z in 0..g { put(&mut w, x, y, z, if z < 3 { stone } else { soil }) }
        put(&mut w, x, y, g, grass);
    }}
    let (tx, ty) = (n / 2, n / 2 + 3);
    let wood_moist = if dead { 100 } else { 900 };
    let leaf_moist = if dead { 80 } else { 1400 };
    let trunk = Cell::solid_of(WOOD, 1000, wood_moist, t, &subs);
    let leaves = Cell::solid_of(LEAVES, 1000, leaf_moist, t, &subs);
    let top = (g + 26).min(nz - 6);
    for z in g + 1..top { for dy in 0..2 { for dx in 0..2 { put(&mut w, tx + dx, ty + dy, z, trunk) } } }
    let c = (tx as i64, ty as i64, (top + 1) as i64);
    for z in (g + 12)..nz { for y in 0..n { for x in 0..n {
        let (dx, dy, dz) = (x as i64 - c.0, y as i64 - c.1, (z as i64 - c.2) * 3 / 2);
        let wobble = noise(seed ^ 9, x as i64 * 3 + z as i64, y as i64 * 3, 4) * 30 / 1000;
        if dx * dx + dy * dy + dz * dz < 50 + wobble && w.get(x, y, z).substance == NONE && hash(seed, x as u64, y as u64, z as u64) % 5 != 0 { put(&mut w, x, y, z, leaves) }
    }}}
    for (bx, by) in [(1i64, 0i64), (-1, 1), (0, -1), (1, 1), (-1, -1)] {
        for k in 1..6i64 { put(&mut w, (c.0 + bx * k) as usize, (c.1 + by * k) as usize, (c.2 - 5 + k / 2) as usize, trunk) }
    }
    // The crib: kindling below, 2×2 dry logs with 2-cell gaps in alternating layers.
    let dry = Cell::solid_of(WOOD, 1000, 100, t, &subs);
    let kindling = Cell::solid_of(KINDLING, 1000, 80, t, &subs);
    let (cx, cy) = (tx - 4, ty - 8);
    let mut torch = Vec::new();
    for y in cy..cy + 8 { for x in cx..cx + 10 { put(&mut w, x, y, g + 1, kindling); if (x + y) % 3 == 0 { torch.push([x, y, g + 1]) } } }
    for layer in 0..4 {
        let z = g + 2 + layer * 2;
        for k in 0..3 { for a in 0..10 { for t2 in 0..2 { for dz in 0..2 {
            let (x, y) = if layer % 2 == 0 { (cx + a, cy + k * 3 + t2) } else { (cx + k * 4 + t2, cy + a.min(7)) };
            put(&mut w, x, y, z + dz, dry);
        }}}}
        // Kindling in the gaps of each layer, as a fire is laid.
        for y in cy..cy + 8 { for x in cx..cx + 10 { if w.get(x, y, z).substance == NONE && (x + y + layer) % 2 == 0 { put(&mut w, x, y, z, kindling) } } }
    }
    bond_all(&mut w, natural_bonds);
    (w, torch)
}

/// A fire scene: the world, where a torch lights it, and the fire's
/// footprint (its corner x, y and width in cells) on grass at z = 4.
pub struct Fire {
    pub world: World,
    pub torch: Vec<(usize, usize, usize)>,
    pub fire: (usize, usize, usize),
}

/// Ground for the fire scenes: stone, soil, a grass top at z = 4, and a stone
/// wall 4 cells behind (−y) the fire at (cx, cy), to catch its light.
fn fire_ground(n: usize, nz: usize, wall: (usize, usize, usize)) -> World {
    let mut w = World::new(n, n, nz, 7, celsius(15));
    let subs = w.subs.clone();
    let t = celsius(15);
    let stone = Cell::solid_of(STONE, 1000, 2, t, &subs);
    let soil = Cell::solid_of(SOIL, 1000, 150, t, &subs);
    let grass = Cell::solid_of(GRASS, 1000, 900, t, &subs);
    for y in 0..n { for x in 0..n { for z in 0..4 { w.set(x, y, z, if z < 2 { stone } else { soil }) } w.set(x, y, 4, grass) } }
    let (x0, x1, y) = wall;
    for z in 5..14.min(nz) { for x in x0..x1.min(n) { put(&mut w, x, y, z, stone) } }
    w
}

/// A small campfire: a 40 cm square of kindling with a layer of kindling and
/// wood checkered over it and two 40 cm logs on top (about 11 kg of wood),
/// a stone wall behind. Lit by a torch at four points.
pub fn campfire(n: usize, nz: usize) -> Fire {
    let (cx, cy) = (n / 2 - 2, n / 2 - 2);
    let mut w = fire_ground(n, nz, (cx - 6, cx + 10, cy - 4));
    let subs = w.subs.clone();
    let t = celsius(15);
    let wood = Cell::solid_of(WOOD, 1000, 100, t, &subs);
    let kindling = Cell::solid_of(KINDLING, 1000, 80, t, &subs);
    for y in cy..cy + 4 { for x in cx..cx + 4 { w.set(x, y, 4, kindling); w.set(x, y, 5, if (x + y) % 2 == 0 { kindling } else { wood }) } }
    for x in cx..cx + 4 { w.set(x, cy, 6, wood); w.set(x, cy + 3, 6, wood) }
    bond_all(&mut w, natural_bonds);
    let torch = vec![(cx, cy, 4), (cx + 2, cy + 1, 4), (cx + 1, cy + 3, 4), (cx + 3, cy + 2, 4)];
    Fire { world: w, torch, fire: (cx, cy, 4) }
}
