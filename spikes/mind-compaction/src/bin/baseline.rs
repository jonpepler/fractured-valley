//! The standing baseline: every variant at every size, printed and written to `results/`.
//!
//! ```text
//! cargo run --release --bin baseline                     # 1k, 10k, 100k, 1M
//! cargo run --release --bin baseline -- --sizes 1k,10k   # a quick look
//! cargo run --release --bin baseline -- --only stack     # variants whose name contains "stack"
//! ```

use fv_mind_compaction::bench::{self, fast_weight_links, Lab, SEED, TICKS};
use fv_mind_compaction::gen::OverlaySpec;
use fv_mind_compaction::{forms, recog, report};
use std::time::Instant;

fn parse_size(s: &str) -> usize {
    let s = s.trim().to_lowercase();
    if let Some(x) = s.strip_suffix('m') { x.parse::<usize>().unwrap() * 1_000_000 } else if let Some(x) = s.strip_suffix('k') { x.parse::<usize>().unwrap() * 1000 } else { s.parse().unwrap() }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let sizes: Vec<usize> = arg("--sizes").map(|s| s.split(',').map(parse_size).collect()).unwrap_or(vec![1000, 10_000, 100_000, 1_000_000]);
    let only = arg("--only");
    let ticks = arg("--ticks").map(|t| t.parse().unwrap()).unwrap_or(TICKS);
    let write = only.is_none() && arg("--ticks").is_none();
    let t_all = Instant::now();
    let mut specs = bench::specs();
    if let Some(o) = &only { specs.retain(|s| s.name.contains(o.as_str()) || s.name == "reference graph"); }
    for s in &mut specs { s.ticks = s.ticks.min(ticks); }

    let (mut rows, mut share, mut rec, mut eps, mut fast, mut gaps, mut gen) = (vec![], vec![], vec![], vec![], vec![], vec![], vec![]);
    for &n in &sizes {
        let lab: Lab = bench::lab(n, SEED, ticks);
        println!("\n== {} nodes: {} nodes, {} links, generated in {} ms", report::size(n), lab.g.len(), lab.g.link_count(), lab.gen_ms);
        gen.push((n, lab.gen_ms, lab.g.len(), lab.g.link_count()));
        let (r, reference) = bench::bench_size(&lab, &specs, |row| println!("{}", report::console_row(row)));
        rows.extend(r);
        if only.is_some() { continue; }
        // Sharing.
        for (minds, delta, spec) in [(100, "2% links, 1% nodes (default)", OverlaySpec::DEFAULT), (1000, "2% links, 1% nodes (default)", OverlaySpec::DEFAULT),
            (100, "0.5% links (young)", OverlaySpec::DEFAULT.scaled(1, 4)), (100, "8% links (old)", OverlaySpec::DEFAULT.scaled(4, 1))] {
            let t0 = Instant::now();
            let s = bench::sharing(&lab, minds, delta, spec);
            println!("sharing {minds} minds ({delta}): base {}, overlay {} per extra mind, page COW {}, alone {} ({} ms)", report::bytes(s.base_bytes), report::bytes(s.overlay_mean), report::bytes(s.page_cow_mean), report::bytes(s.independent_per_mind), t0.elapsed().as_millis());
            share.push(s);
        }
        fast.push((n, lab.g.link_count(), fast_weight_links(&lab, &reference)));
        let shuf = lab.g.permute(&forms::shuffled(lab.g.len(), SEED));
        let rcm = shuf.permute(&forms::rcm(&shuf));
        gaps.push((n, forms::mean_gap(&lab.g), forms::mean_gap(&shuf), forms::mean_gap(&rcm)));
        // Bit vectors where they might win.
        let things = (n * 12 / 100).max(200);
        let r = recog::recognition(things, 500, SEED);
        for x in &r { println!("recognition, {} things: {:<55} {:>10} {:>9} ns  identify {:>5}‰ category {:>5}‰ food {:>5}‰", x.things, x.method, report::bytes(x.bytes), x.ns_query, x.id_pm, x.cat_pm, x.food_pm); }
        rec.push((n, r));
        let episodes: Vec<Vec<(u32, i32)>> = reference.ws.iter().zip(&reference.act).map(|(ws, act)| ws.iter().map(|&w| (w, act.binary_search_by_key(&w, |a| a.0).map(|i| act[i].1).unwrap_or(0))).collect()).collect();
        let e = recog::episodes(&episodes, lab.g.len(), 300, SEED);
        for x in &e { println!("episodes, {}: {:<40} {:>5} B {:>9} ns  exact {:>5}‰ quality {:>5}‰", x.episodes, x.method, x.bytes_per, x.ns_query, x.exact_pm, x.quality_pm); }
        eps.push((n, e));
    }
    println!("\nall done in {:.1} s", t_all.elapsed().as_secs_f64());
    if !write { println!("(--only/--ticks given: results/ not written)"); return; }
    let cpu = std::fs::read_to_string("/proc/cpuinfo").ok().and_then(|c| c.lines().find(|l| l.starts_with("model name")).map(|l| l.split(':').nth(1).unwrap_or("").trim().to_string())).unwrap_or_else(|| "unknown CPU".into());
    let threads = std::thread::available_parallelism().map(|x| x.get()).unwrap_or(1);
    let header = format!("Machine: {cpu}, {threads} threads; one thread per measurement. Seed `{SEED:#x}`, {ticks} ticks per size. Cache misses: from the CPU's counters where the machine exposes them (`n/a`: no PMU, as in most VMs). Whole run: {:.0} s.", t_all.elapsed().as_secs_f64());
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/results");
    std::fs::create_dir_all(dir).unwrap();
    let x = report::Extra { share: &share, recog: &rec, episodes: &eps, fast: &fast, gaps: &gaps, gen_ms: &gen };
    std::fs::write(format!("{dir}/baseline.md"), report::markdown(&rows, &x, &header)).unwrap();
    std::fs::write(format!("{dir}/baseline.csv"), report::csv(&rows)).unwrap();
    println!("wrote {dir}/baseline.md and baseline.csv");
}
