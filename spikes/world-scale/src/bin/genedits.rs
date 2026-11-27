//! A 1 km × 1 km valley at 10 cm: the world as generator(seed) + stored edits.
//!
//!     cargo run --release --bin genedits
//!
//! Measures:
//! 1. How many 8³ bricks the valley has, and how many hold its surface
//!    (both solid and air): the part that isn't trivially uniform.
//! 2. What storing it would cost: dense, sparse surface bricks with a
//!    per-brick palette, and generator + edits (nothing stored until edited).
//! 3. Generator speed: ns per cell to materialise a surface brick, and what
//!    streaming costs a player running through the valley.
//! 4. Edits after a long game, as a canonical (sorted, hashable) log,
//!    raw and deflated.
//!
//! Only materials are edited here. Heat, gas and the other live state of a
//! region that goes quiet are handed to the coarse level (brief 10, D2), so
//! they don't sit in the edit log.
use std::collections::BTreeMap;
use std::time::Instant;

const SEED: u64 = 0x5eed_0f_7a11e7;
const B: i64 = 8; // brick side, cells (as fv-core)
const W: i64 = 10_000; // cells across (1 km at 10 cm)
const BW: i64 = W / B;
const DEPTH: i64 = 640; // cells of world height modelled (64 m)

const AIR: u8 = 0;
const STONE: u8 = 1;
const SOIL: u8 = 2;
const GRASS: u8 = 3;
const ASH: u8 = 4;
const WOOD: u8 = 5;
const ORE: u8 = 6;
const WATER: u8 = 7;

fn hash(seed: u64, x: u64, y: u64, z: u64) -> u64 {
    let mut h = seed ^ x.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ y.wrapping_mul(0xc2b2_ae3d_27d4_eb4f) ^ z.wrapping_mul(0x1656_67b1_9e37_79f9);
    h ^= h >> 33; h = h.wrapping_mul(0xff51_afd7_ed55_8ccd); h ^= h >> 33; h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53); h ^ (h >> 33)
}

/// Integer value noise, 0..1000 (as fv-core's `scene::noise`).
fn noise(seed: u64, x: i64, y: i64, scale: i64) -> i64 {
    let (gx, gy) = (x.div_euclid(scale), y.div_euclid(scale));
    let (fx, fy) = (x.rem_euclid(scale) * 1000 / scale, y.rem_euclid(scale) * 1000 / scale);
    let v = |a: i64, b: i64| (hash(seed, a as u64, b as u64, 0x7e1) % 1000) as i64;
    let s = |t: i64| t * t * (3000 - 2 * t) / 1_000_000;
    let (sx, sy) = (s(fx), s(fy));
    let top = v(gx, gy) * (1000 - sx) + v(gx + 1, gy) * sx;
    let bot = v(gx, gy + 1) * (1000 - sx) + v(gx + 1, gy + 1) * sx;
    (top * (1000 - sy) + bot * sy) / 1_000_000
}

/// Ground height (cells) at a column: a valley along y with a river at the
/// bottom, hills either side, 4 octaves of detail down to 3 m.
fn height(x: i64, y: i64) -> i64 {
    let across = (x - W / 2 - (noise(SEED ^ 1, 0, y, 2_000) - 500) * 3).abs(); // meandering floor
    let valley = 120 + across * 250 / (W / 2); // 12 m at the floor to 37 m at the rim
    let detail = (noise(SEED, x, y, 2_400) - 500) * 80 / 1000 + (noise(SEED ^ 2, x, y, 600) - 500) * 30 / 1000
        + (noise(SEED ^ 3, x, y, 150) - 500) * 8 / 1000 + (noise(SEED ^ 4, x, y, 30) - 500) * 2 / 1000;
    (valley + detail).clamp(20, DEPTH - 40)
}
const RIVER_LEVEL: i64 = 118;

fn material(x: i64, y: i64, z: i64, h: i64) -> u8 {
    if z > h { return if z <= RIVER_LEVEL { WATER } else { AIR } }
    if z == h { return if h <= RIVER_LEVEL { SOIL } else { GRASS } }
    if z > h - 8 { return SOIL }
    if hash(SEED ^ 9, x as u64, y as u64, z as u64) % 200 == 0 { return ORE }
    STONE
}

fn materialise(bx: i64, by: i64, bz: i64, out: &mut [u8; 512]) {
    for y in 0..B { for x in 0..B {
        let (wx, wy) = (bx * B + x, by * B + y);
        let h = height(wx, wy);
        for z in 0..B { out[((z * B + y) * B + x) as usize] = material(wx, wy, bz * B + z, h) }
    }}
}

/// Brick layers each brick column's surface spans (bricks holding both solid
/// and non-solid), from the corner and centre heights (a sample of 9 columns,
/// checked against full materialisation on a sample below).
fn surface_layers(bx: i64, by: i64) -> (i64, i64) {
    let (mut lo, mut hi) = (i64::MAX, i64::MIN);
    for y in [0, B / 2, B - 1] { for x in [0, B / 2, B - 1] {
        let h = height(bx * B + x, by * B + y);
        lo = lo.min(h); hi = hi.max(h.max(RIVER_LEVEL));
    }}
    (lo.div_euclid(B), hi.div_euclid(B))
}

fn palette_bytes(cells: &[u8; 512]) -> usize {
    let mut seen = [false; 256];
    for &c in cells { seen[c as usize] = true }
    let n = seen.iter().filter(|&&s| s).count();
    if n == 1 { return 8 } // uniform: header only
    let bits = (usize::BITS - (n - 1).leading_zeros()) as usize;
    8 + n + 512 * bits / 8
}

#[derive(Default)]
struct Edits { bricks: BTreeMap<(i64, i64, i64), BTreeMap<u16, u8>> }
impl Edits {
    fn set(&mut self, x: i64, y: i64, z: i64, m: u8) {
        if x < 0 || y < 0 || x >= W || y >= W || z < 0 || z >= DEPTH { return }
        let k = (x.div_euclid(B), y.div_euclid(B), z.div_euclid(B));
        let i = ((z.rem_euclid(B) * B + y.rem_euclid(B)) * B + x.rem_euclid(B)) as u16;
        let gen = material(x, y, z, height(x, y));
        let b = self.bricks.entry(k).or_default();
        if m == gen { b.remove(&i); } else { b.insert(i, m); }
    }
    fn cells(&self) -> usize { self.bricks.values().map(|b| b.len()).sum() }
    /// Canonical bytes: per brick, its key, then either a 512-bit changed
    /// mask + one byte per changed cell, or (if denser) all 512 bytes.
    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for (k, b) in &self.bricks {
            if b.is_empty() { continue }
            for v in [k.0 as u16, k.1 as u16, k.2 as u16] { out.extend_from_slice(&v.to_le_bytes()) }
            if b.len() * 8 > 512 * 7 {
                out.push(1);
                let mut full = [0u8; 512];
                materialise(k.0, k.1, k.2, &mut full);
                for (&i, &m) in b { full[i as usize] = m }
                out.extend_from_slice(&full);
            } else {
                out.push(0);
                let mut mask = [0u8; 64];
                for &i in b.keys() { mask[i as usize / 8] |= 1 << (i % 8) }
                out.extend_from_slice(&mask);
                out.extend(b.values());
            }
        }
        out
    }
}

fn mib(b: f64) -> String { if b >= 1024.0 * 1024.0 * 1024.0 { format!("{:.2} GiB", b / (1u64 << 30) as f64) } else { format!("{:.2} MiB", b / (1u64 << 20) as f64) } }

fn main() {
    // 1. Bricks.
    let t0 = Instant::now();
    let mut surface = 0i64;
    let mut sample = Vec::new();
    for by in 0..BW { for bx in 0..BW {
        let (lo, hi) = surface_layers(bx, by);
        surface += hi - lo + 1;
        if hash(SEED ^ 5, bx as u64, by as u64, 0) % 500 == 0 { sample.push((bx, by, lo, hi)) }
    }}
    let scan_ms = t0.elapsed().as_millis();
    let total_bricks = BW * BW * (DEPTH / B);

    // Palette sizes and a check of the surface estimate, on the sample.
    let mut buf = [0u8; 512];
    let (mut pal, mut n_pal, mut mixed, mut est) = (0usize, 0usize, 0i64, 0i64);
    let t1 = Instant::now();
    for &(bx, by, lo, hi) in &sample {
        est += hi - lo + 1;
        for bz in (lo - 1).max(0)..=(hi + 1).min(DEPTH / B - 1) {
            materialise(bx, by, bz, &mut buf);
            let solid = buf.iter().filter(|&&c| c != AIR && c != WATER).count();
            if solid > 0 && solid < 512 { mixed += 1 }
            if bz >= lo && bz <= hi { pal += palette_bytes(&buf); n_pal += 1 }
        }
    }
    let gen_cells = sample.iter().map(|&(_, _, lo, hi)| ((hi + 1).min(DEPTH / B - 1) - (lo - 1).max(0) + 1) as usize * 512).sum::<usize>();
    let gen_ns = t1.elapsed().as_nanos() as f64 / gen_cells as f64;
    let pal_per = pal as f64 / n_pal as f64;

    println!("## 1. The valley\n");
    println!("1 km × 1 km × 64 m at 10 cm: {:.1}×10⁹ cells, {} bricks of 8³. Floor 12 m, rim ~37 m, river at 11.8 m.\n", (W * W * DEPTH) as f64 / 1e9, total_bricks);
    println!("| | count |\n|---|---|");
    println!("| all bricks | {} |", total_bricks);
    println!("| surface bricks (span ground level in some column) | {} ({:.2} %) |", surface, 100.0 * surface as f64 / total_bricks as f64);
    println!("| check on a {}-column sample: bricks really mixed solid/air vs estimated | {} vs {} |", sample.len(), mixed, est);
    println!("| scanning every column's surface | {} ms |", scan_ms);

    println!("\n## 2. Storing it\n");
    println!("| representation | bytes |\n|---|---|");
    println!("| dense, fv-core today (340 B/cell resident) | {} |", mib((W * W * DEPTH) as f64 * 340.0));
    println!("| dense, 1 byte material per cell | {} |", mib((W * W * DEPTH) as f64));
    println!("| sparse surface bricks, materials only, per-brick palette (mean {:.0} B/brick) | {} |", pal_per, mib(surface as f64 * pal_per));
    println!("| generator + edits, unedited | 0 (the seed) |");

    println!("\n## 3. Generating\n");
    println!("Materialising surface bricks: **{:.1} ns per cell**, {:.1} µs per brick, one thread ({} cells).", gen_ns, gen_ns * 512.0 / 1000.0, gen_cells);
    let bricks_per_col = surface as f64 / (BW * BW) as f64;
    for (r, v) in [(32.0f64, 6.0f64), (64.0, 6.0), (64.0, 20.0)] {
        // New area per second: the leading edge of a disc, 2r wide, moving at v.
        let cols = 2.0 * r * v / (0.8 * 0.8);
        let bricks = cols * (bricks_per_col + 2.0); // surface plus a brick of air and ground each side
        let ms_per_s = bricks * gen_ns * 512.0 / 1e6;
        println!("- Active radius {r} m, moving {v} m/s: {:.0} new bricks/s, **{:.1} ms of generation per second** ({:.2} ms per 50 ms tick).", bricks, ms_per_s, ms_per_s / 20.0);
    }

    // 4. Edits after a long game.
    let mut e = Edits::default();
    let mut rows = Vec::new();
    let record = |name: &str, e: &Edits, rows: &mut Vec<(String, usize, usize, usize, usize)>| {
        let raw = e.encode();
        let z = miniz_oxide::deflate::compress_to_vec(&raw, 6);
        rows.push((name.to_string(), e.cells(), e.bricks.values().filter(|b| !b.is_empty()).count(), raw.len(), z.len()));
    };
    // A: 10 000 digs and placements (1 m diameter holes, or 1 m³ of placed stone).
    for i in 0..10_000u64 {
        let x = (hash(SEED ^ 11, i, 0, 0) % W as u64) as i64;
        let y = (hash(SEED ^ 11, i, 1, 0) % W as u64) as i64;
        let h = height(x, y);
        let place = i % 3 == 0;
        for dz in -5..5 { for dy in -5..5 { for dx in -5..5 {
            if dx * dx + dy * dy + dz * dz > 25 { continue }
            if place { e.set(x + dx, y + dy, h + 6 + dz, STONE) } else { e.set(x + dx, y + dy, h + dz, AIR) }
        }}}
    }
    record("10 000 digs and placements (~0.5 m³ each)", &e, &mut rows);
    // B: 40 buildings, 8 × 6 × 4 m, 20 cm wooden walls, floors and roofs.
    for i in 0..40u64 {
        let x0 = 4_000 + (hash(SEED ^ 12, i, 0, 0) % 2_000) as i64;
        let y0 = (hash(SEED ^ 12, i, 1, 0) % (W as u64 - 100)) as i64;
        let h = height(x0, y0) + 1;
        for z in 0..40 { for y in 0..60 { for x in 0..80 {
            let wall = x < 2 || x >= 78 || y < 2 || y >= 58 || z < 2 || z >= 38;
            e.set(x0 + x, y0 + y, h + z, if wall { WOOD } else { AIR });
        }}}
    }
    record("+ 40 buildings, 8 × 6 × 4 m", &e, &mut rows);
    // C: a 10 ha fire: the grass layer of a 316 m square burnt to ash.
    for y in 3_000..6_160 { for x in 3_400..6_560 {
        let h = height(x, y);
        if material(x, y, h, h) == GRASS { e.set(x, y, h, ASH) }
    }}
    record("+ a 10 ha grass fire (surface layer to ash)", &e, &mut rows);
    // D: a felled forest: 2 000 trees cut, trunks 30 cm × 6 m removed (as if
    // the generator had placed them; here stored as placed-then-cut logs).
    for i in 0..2_000u64 {
        let x = (hash(SEED ^ 13, i, 0, 0) % W as u64) as i64;
        let y = (hash(SEED ^ 13, i, 1, 0) % W as u64) as i64;
        let h = height(x, y);
        for z in 1..4 { for dy in 0..3 { for dx in 0..20 { e.set(x + dx, y + dy, h + z, WOOD) } } } // a log on the ground
    }
    record("+ 2 000 logs left lying (30 cm × 2 m)", &e, &mut rows);

    println!("\n## 4. Edits after a long game (cumulative)\n");
    println!("| edits | cells changed | bricks touched | log, raw | log, deflated | deflated, per changed cell |\n|---|---|---|---|---|---|");
    for (name, cells, bricks, raw, z) in rows {
        println!("| {name} | {cells} | {bricks} | {} | {} | {:.2} B |", mib(raw as f64), mib(z as f64), z as f64 / cells as f64);
    }
}
