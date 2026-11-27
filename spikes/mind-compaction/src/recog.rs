//! Bit vectors where they might actually win: recognition and episode recall, not the whole
//! mind. Each task is done both ways, the graph way and the hyperdimensional way, on the same
//! data, and scored against the truth and against the exact answer.
//!
//! **Recognition** ("what is this, and does it look like food?"). A world of perceptual
//! features: 1024 feature nodes, 8 categories (food is category 0), each with a prototype of 12
//! features. A known thing has 6 of its category's prototype features and 4 of its own. A
//! percept of a thing loses 30% of its features and gains 2 random ones.
//! - Graph, identify: an inverted index feature → things (the reverse links a backward spread
//!   would follow); count shared features, best count wins (ties to the lower id). Exact.
//! - Graph, categorise: one hop features → category concepts, weighted by how many members
//!   have the feature (a feature × category table).
//! - Bit vectors, identify: a D-bit signature per thing (majority bundle of its features'
//!   seeded item vectors); scan for the nearest by Hamming distance.
//! - Bit vectors, categorise: one prototype per category (a bundle of its members' signatures);
//!   nearest of 8.
//!
//! **Episodes** ("have I been here before?"). Every tick's workspace of the reference run is an
//! episode (8 nodes with activation). A probe is a stored workspace with 2 of its 8 nodes swapped
//! for random ones. The exact answer is the episode with the largest activation-weighted overlap.
//! - List: 8 × (u32 id, u8 activation) per episode; overlap by merging sorted ids.
//! - Bit vectors: a D-bit bundle per episode, each node's item repeated by activation rank;
//!   nearest by Hamming distance.

use crate::rng::hash;
use std::time::Instant;

const FEATURES: u32 = 1024;
const CATS: usize = 8;

fn item(seed: u64, f: u32, w: usize) -> u64 { hash(seed, f as u64, w as u64, 0x5e75) }

/// Majority bundle of items (each with a repeat count), D = `words` × 64 bits; ties to a seeded bit.
fn bundle(seed: u64, members: &[(u32, i32)], words: usize, out: &mut [u64], counts: &mut Vec<i32>) {
    counts.clear();
    counts.resize(words * 64, 0);
    for &(f, c) in members {
        for (w, chunk) in counts.chunks_mut(64).enumerate() {
            let x = item(seed, f, w);
            for (b, slot) in chunk.iter_mut().enumerate() { *slot += if (x >> b) & 1 == 1 { c } else { -c }; }
        }
    }
    for (w, o) in out.iter_mut().enumerate() {
        let tie = hash(seed, w as u64, 0, 0x71e);
        let mut x = 0u64;
        for b in 0..64 { let c = counts[w * 64 + b]; if c > 0 || (c == 0 && (tie >> b) & 1 == 1) { x |= 1 << b; } }
        *o = x;
    }
}
#[inline]
fn ham(a: &[u64], b: &[u64]) -> u32 { a.iter().zip(b).map(|(x, y)| (x ^ y).count_ones()).sum() }

#[derive(Clone, Debug)]
pub struct RecogRow { pub things: usize, pub method: String, pub bytes: usize, pub ns_query: u64, pub id_pm: u32, pub cat_pm: u32, pub food_pm: u32 }

pub fn recognition(things: usize, queries: usize, seed: u64) -> Vec<RecogRow> {
    let h = |a: u64, b: u64, c: u64| hash(seed, a, b, c);
    let proto: Vec<Vec<u32>> = (0..CATS as u64).map(|c| (0..12).map(|k| (h(c, k, 1) % FEATURES as u64) as u32).collect()).collect();
    let mut feats: Vec<Vec<u32>> = Vec::with_capacity(things);
    let mut cat = Vec::with_capacity(things);
    for t in 0..things as u64 {
        let c = (h(t, 0, 2) % CATS as u64) as usize;
        let mut f: Vec<u32> = (0..6).map(|k| proto[c][(h(t, k, 3) % 12) as usize]).chain((0..4).map(|k| (h(t, k, 4) % FEATURES as u64) as u32)).collect();
        f.sort_unstable(); f.dedup();
        feats.push(f);
        cat.push(c);
    }
    let qs: Vec<(usize, Vec<u32>)> = (0..queries as u64).map(|q| {
        let t = (h(q, 0, 5) % things as u64) as usize;
        let mut p: Vec<u32> = feats[t].iter().copied().filter(|&f| h(q, f as u64, 6) % 100 >= 30).collect();
        p.push((h(q, 1, 7) % FEATURES as u64) as u32);
        p.push((h(q, 2, 7) % FEATURES as u64) as u32);
        p.sort_unstable(); p.dedup();
        (t, p)
    }).collect();
    let mut rows = vec![];
    let score = |name: String, bytes: usize, ns: u64, ans: &[(usize, usize)]| -> RecogRow {
        let n = ans.len().max(1) as u32;
        let id = ans.iter().zip(&qs).filter(|(a, q)| a.0 == q.0).count() as u32;
        let ca = ans.iter().zip(&qs).filter(|(a, q)| a.1 == cat[q.0]).count() as u32;
        let food = ans.iter().zip(&qs).filter(|(a, q)| (a.1 == 0) == (cat[q.0] == 0)).count() as u32;
        RecogRow { things, method: name, bytes, ns_query: ns, id_pm: id * 1000 / n, cat_pm: ca * 1000 / n, food_pm: food * 1000 / n }
    };
    // Graph: inverted index and a feature × category table.
    let mut off = vec![0u32; FEATURES as usize + 1];
    for f in &feats { for &x in f { off[x as usize + 1] += 1; } }
    for i in 0..FEATURES as usize { off[i + 1] += off[i]; }
    let mut fill = off.clone();
    let mut inv = vec![0u32; off[FEATURES as usize] as usize];
    for (t, f) in feats.iter().enumerate() { for &x in f { inv[fill[x as usize] as usize] = t as u32; fill[x as usize] += 1; } }
    let mut table = vec![[0u16; CATS]; FEATURES as usize];
    let mut members = [0u32; CATS];
    for &c in &cat { members[c] += 1; }
    let mut raw = vec![[0u32; CATS]; FEATURES as usize];
    for (t, f) in feats.iter().enumerate() { for &x in f { raw[x as usize][cat[t]] += 1; } }
    for x in 0..FEATURES as usize { for c in 0..CATS { table[x][c] = (raw[x][c] * 1000 / members[c].max(1)) as u16; } }
    let mut counts = vec![0u16; things];
    let mut touched: Vec<u32> = vec![];
    let t0 = Instant::now();
    let ans: Vec<(usize, usize)> = qs.iter().map(|(_, p)| {
        for &x in p { for &t in &inv[off[x as usize] as usize..off[x as usize + 1] as usize] { if counts[t as usize] == 0 { touched.push(t); } counts[t as usize] += 1; } }
        let best = touched.iter().copied().max_by_key(|&t| (counts[t as usize], std::cmp::Reverse(t))).unwrap_or(0) as usize;
        for &t in &touched { counts[t as usize] = 0; }
        touched.clear();
        (best, cat[best])
    }).collect();
    let ns = t0.elapsed().as_nanos() as u64 / queries as u64;
    rows.push(score("graph: inverted index (identify, its category)".into(), off.len() * 4 + inv.len() * 4 + things, ns, &ans));
    let t0 = Instant::now();
    let ans: Vec<(usize, usize)> = qs.iter().map(|(_, p)| {
        let mut s = [0u32; CATS];
        for &x in p { for c in 0..CATS { s[c] += table[x as usize][c] as u32; } }
        let c = (0..CATS).max_by_key(|&c| (s[c], std::cmp::Reverse(c))).unwrap();
        (usize::MAX, c)
    }).collect();
    let ns = t0.elapsed().as_nanos() as u64 / queries as u64;
    rows.push(score("graph: features → category (categorise only)".into(), FEATURES as usize * CATS * 2, ns, &ans));
    // Bit vectors.
    let mut cnt = vec![];
    for bits in [256usize, 1024] {
        let words = bits / 64;
        let mut sig = vec![0u64; things * words];
        let t0 = Instant::now();
        for (t, f) in feats.iter().enumerate() {
            let m: Vec<(u32, i32)> = f.iter().map(|&x| (x, 1)).collect();
            bundle(seed, &m, words, &mut sig[t * words..(t + 1) * words], &mut cnt);
        }
        let _build = t0.elapsed();
        let mut q = vec![0u64; words];
        let t0 = Instant::now();
        let ans: Vec<(usize, usize)> = qs.iter().map(|(_, p)| {
            let m: Vec<(u32, i32)> = p.iter().map(|&x| (x, 1)).collect();
            bundle(seed, &m, words, &mut q, &mut cnt);
            let best = (0..things).min_by_key(|&t| (ham(&q, &sig[t * words..(t + 1) * words]), t)).unwrap();
            (best, cat[best])
        }).collect();
        let ns = t0.elapsed().as_nanos() as u64 / queries as u64;
        rows.push(score(format!("bit vectors {bits}: nearest signature (identify)"), sig.len() * 8 + things, ns, &ans));
        // Prototypes: bundle of the category's members' features (weighted by how often).
        let mut protos = vec![0u64; CATS * words];
        for c in 0..CATS {
            let mut m: Vec<(u32, i32)> = vec![];
            for x in 0..FEATURES { let r = raw[x as usize][c] as i32; if r > 0 { m.push((x, r)); } }
            bundle(seed, &m, words, &mut protos[c * words..(c + 1) * words], &mut cnt);
        }
        let t0 = Instant::now();
        let ans: Vec<(usize, usize)> = qs.iter().map(|(_, p)| {
            let m: Vec<(u32, i32)> = p.iter().map(|&x| (x, 1)).collect();
            bundle(seed, &m, words, &mut q, &mut cnt);
            let c = (0..CATS).min_by_key(|&c| (ham(&q, &protos[c * words..(c + 1) * words]), c)).unwrap();
            (usize::MAX, c)
        }).collect();
        let ns = t0.elapsed().as_nanos() as u64 / queries as u64;
        rows.push(score(format!("bit vectors {bits}: nearest prototype (categorise only)"), protos.len() * 8, ns, &ans));
    }
    rows
}

#[derive(Clone, Debug)]
pub struct EpisodeRow { pub episodes: usize, pub method: String, pub bytes_per: usize, pub ns_query: u64, pub exact_pm: u32, pub quality_pm: u32 }

/// `eps`: workspaces with activation (from the reference run), `n`: node count of the mind.
pub fn episodes(eps: &[Vec<(u32, i32)>], n: usize, probes: usize, seed: u64) -> Vec<EpisodeRow> {
    let h = |a: u64, b: u64| hash(seed, a, b, 0xe915);
    let eps: Vec<Vec<(u32, i32)>> = eps.iter().filter(|e| !e.is_empty()).map(|e| { let mut e = e.clone(); e.sort_unstable(); e }).collect();
    let probes_v: Vec<Vec<(u32, i32)>> = (0..probes as u64).map(|q| {
        let mut e = eps[(h(q, 1) % eps.len() as u64) as usize].clone();
        for k in 0..2 { if !e.is_empty() { let i = (h(q, 2 + k) % e.len() as u64) as usize; e[i].0 = (h(q, 9 + k) % n as u64) as u32; } }
        e.sort_unstable(); e.dedup_by_key(|x| x.0);
        e
    }).collect();
    // Quantised list: activation in one byte (/16).
    let q8 = |e: &[(u32, i32)]| -> Vec<(u32, i32)> { e.iter().map(|&(id, a)| (id, (a / 16).clamp(1, 255))).collect() };
    let overlap = |a: &[(u32, i32)], b: &[(u32, i32)]| -> i64 {
        let (mut i, mut j, mut s) = (0, 0, 0i64);
        while i < a.len() && j < b.len() {
            if a[i].0 == b[j].0 { s += a[i].1.min(b[j].1) as i64; i += 1; j += 1; } else if a[i].0 < b[j].0 { i += 1 } else { j += 1 }
        }
        s
    };
    // The exact answer(s): the best weighted overlap at full precision (any tie counts).
    let best_score: Vec<i64> = probes_v.iter().map(|p| eps.iter().map(|e| overlap(p, e)).max().unwrap_or(0)).collect();
    let mut rows = vec![];
    let exact = |picks: &[usize]| picks.iter().zip(&probes_v).zip(&best_score).filter(|((&i, p), &b)| overlap(p, &eps[i]) == b).count() as u32 * 1000 / probes as u32;
    let quality = |picks: &[usize]| (picks.iter().zip(&probes_v).zip(&best_score).map(|((&i, p), &b)| if b > 0 { overlap(p, &eps[i]) * 1000 / b } else { 1000 }).sum::<i64>() / probes as i64) as u32;
    for (name, bytes_per, quant) in [("list: 8 × (u32 id, u16 activation)", 8 * 6, false), ("list: 8 × (u32 id, u8 activation)", 8 * 5, true)] {
        let lists: Vec<Vec<(u32, i32)>> = eps.iter().map(|e| if quant { q8(e) } else { e.clone() }).collect();
        let t0 = Instant::now();
        let picks: Vec<usize> = probes_v.iter().map(|p| { let p = if quant { q8(p) } else { p.clone() }; (0..lists.len()).max_by_key(|&i| (overlap(&p, &lists[i]), std::cmp::Reverse(i))).unwrap() }).collect();
        let ns = t0.elapsed().as_nanos() as u64 / probes as u64;
        rows.push(EpisodeRow { episodes: eps.len(), method: name.into(), bytes_per, ns_query: ns, exact_pm: exact(&picks), quality_pm: quality(&picks) });
    }
    let mut cnt = vec![];
    for bits in [128usize, 256, 1024] {
        let words = bits / 64;
        let enc = |e: &[(u32, i32)], out: &mut [u64], cnt: &mut Vec<i32>| {
            let mut r: Vec<(u32, i32)> = e.to_vec();
            r.sort_by_key(|x| (std::cmp::Reverse(x.1), x.0));
            let m: Vec<(u32, i32)> = r.iter().enumerate().map(|(k, x)| (x.0, (8 - k as i32).max(1))).collect();
            bundle(seed, &m, words, out, cnt);
        };
        let mut sig = vec![0u64; eps.len() * words];
        for (i, e) in eps.iter().enumerate() { enc(e, &mut sig[i * words..(i + 1) * words], &mut cnt); }
        let mut q = vec![0u64; words];
        let t0 = Instant::now();
        let picks: Vec<usize> = probes_v.iter().map(|p| { enc(p, &mut q, &mut cnt); (0..eps.len()).min_by_key(|&i| (ham(&q, &sig[i * words..(i + 1) * words]), i)).unwrap() }).collect();
        let ns = t0.elapsed().as_nanos() as u64 / probes as u64;
        rows.push(EpisodeRow { episodes: eps.len(), method: format!("bit vectors {bits}"), bytes_per: bits / 8, ns_query: ns, exact_pm: exact(&picks), quality_pm: quality(&picks) });
    }
    rows
}
