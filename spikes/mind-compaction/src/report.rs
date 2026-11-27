//! Formatting: the console table, `results/baseline.md` and `results/baseline.csv`.

use crate::bench::{Faith, Row, ShareRow};
use crate::recog::{EpisodeRow, RecogRow};
use std::fmt::Write;

pub fn size(n: usize) -> String {
    if n >= 1_000_000 && n % 1_000_000 == 0 { format!("{}M", n / 1_000_000) } else if n >= 1000 && n % 1000 == 0 { format!("{}k", n / 1000) } else { n.to_string() }
}
pub fn bytes(b: usize) -> String {
    if b >= 10 << 20 { format!("{:.1} MB", b as f64 / 1048576.0) } else if b >= 10 << 10 { format!("{:.1} KB", b as f64 / 1024.0) } else { format!("{b} B") }
}
fn per_node(r: &Row) -> String { format!("{:.1}", r.bytes as f64 / r.nodes as f64) }
fn us(ns: u64) -> String { if ns >= 100_000 { format!("{:.0}", ns as f64 / 1000.0) } else { format!("{:.1}", ns as f64 / 1000.0) } }
fn pm(x: u32) -> String { format!("{:.1}%", x as f64 / 10.0) }
/// Strict, then tie-aware (shown only when it differs).
fn pm2(x: u32, t: u32) -> String { if x == t { pm(x) } else { format!("{:.1}% ({:.1}%)", x as f64 / 10.0, t as f64 / 10.0) } }
fn faith(f: Faith) -> &'static str { match f { Faith::Exact => "exact", Faith::UpToTies => "up to ties", Faith::Floor => "faithful", Faith::Explore => "explore" } }

pub fn console_row(r: &Row) -> String {
    let g = r.agree;
    format!("{:>4} {:<42} {:>7} B/node {:>9} {:>6} µs mean {:>6} p99 {:>5} links  top1 {:>15} ws {:>15} act {:>15} regret {:>3}‰ err {:>4}‰ {}{}",
        size(r.n), r.name, per_node(r), bytes(r.bytes), us(r.ns_mean), us(r.ns_p99), r.visits, pm2(g.top1_pm, g.top1_tie_pm), pm2(g.ws_pm, g.ws_tie_pm), pm2(g.action_pm, g.action_tie_pm), g.regret_pm, g.act_err_pm,
        if r.exact { "exact" } else { "" }, if r.note.is_empty() { String::new() } else { format!(" ({})", r.note) })
}

pub fn csv(rows: &[Row]) -> String {
    let mut s = String::from("nodes,links,variant,family,intent,bytes_per_mind,bytes_per_node,shared_bytes,state_bytes,build_ms,ns_mean,ns_p50,ns_p99,cache_misses_per_think,links_per_think,top1_pm,ws_overlap_pm,action_pm,act_err_pm,top1_tie_pm,ws_overlap_tie_pm,action_tie_pm,regret_pm,exact,hash,ticks,note\n");
    for r in rows {
        let _ = writeln!(s, "{},{},\"{}\",{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{:#018x},{},\"{}\"",
            r.nodes, r.links, r.name, r.family, faith(r.faith), r.bytes, per_node(r), r.shared, r.state, r.build_ms, r.ns_mean, r.ns_p50, r.ns_p99,
            r.misses.map(|m| m.to_string()).unwrap_or_else(|| "n/a".into()), r.visits, r.agree.top1_pm, r.agree.ws_pm, r.agree.action_pm, r.agree.act_err_pm, r.agree.top1_tie_pm, r.agree.ws_tie_pm, r.agree.action_tie_pm, r.agree.regret_pm, r.exact, r.hash, r.ticks, r.note);
    }
    s
}

pub struct Extra<'a> {
    pub share: &'a [ShareRow],
    pub recog: &'a [(usize, Vec<RecogRow>)],
    pub episodes: &'a [(usize, Vec<EpisodeRow>)],
    /// (nodes, links, distinct fast-weight links touched over the run, mean |source − target| natural, shuffled, RCM)
    pub fast: &'a [(usize, usize, usize)],
    pub gaps: &'a [(usize, u64, u64, u64)],
    pub gen_ms: &'a [(usize, u128, usize, usize)],
}

pub fn markdown(rows: &[Row], x: &Extra, header: &str) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "# Mind compaction: the standing baseline\n\n{header}\n");
    let _ = writeln!(s, "Regenerate with `cargo run --release --bin baseline` (in `spikes/mind-compaction`). Every number below is from that run.\n");
    let _ = writeln!(s, "**Columns.** *B/node* and *per mind*: structure bytes for one mind (shared parts apart). *State*: bytes kept per mind between thinks. *µs*: per think, mean and p99. *Links*: links read per think. *Agreement* is against the reference over the same {} ticks: same top thought, workspace (top-8) overlap, same action on the fixed decision rule, and activation error (Σ|Δ| / Σ reference). In brackets, where different: the tie-aware figure, which also counts a pick the reference ranks equal to its own (integer activations tie often and the reference breaks ties by node id, so a renumbered mind can pick a different, equally ranked node). *Regret*: how much worse the variant's choice is under the reference's own scores, per mille of the reference's best (the decision rule is a close call on many ticks, so *Action* flips on small activation changes while *Regret* stays small). *Exact*: the run's hash equals the reference's.\n", rows.first().map(|r| r.ticks).unwrap_or(0));
    let _ = writeln!(s, "## Minds\n\n| Nodes | Links | Links/node | Generate + overlay + workload (ms) |\n|---|---|---|---|");
    for &(n, ms, nodes, links) in x.gen_ms { let _ = writeln!(s, "| {} ({nodes}) | {links} | {:.1} | {ms} |", size(n), links as f64 / nodes as f64); }
    let mut sizes: Vec<usize> = rows.iter().map(|r| r.n).collect();
    sizes.dedup();
    for n in sizes {
        let _ = writeln!(s, "\n## {} nodes\n", size(n));
        let _ = writeln!(s, "| Variant | Family | Intent | B/node | Per mind | Shared | State | Build ms | µs mean | µs p99 | Cache misses | Links | Top-1 | WS overlap | Action | Regret ‰ | Act err ‰ | Exact | Note |");
        let _ = writeln!(s, "|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|");
        for r in rows.iter().filter(|r| r.n == n) {
            let _ = writeln!(s, "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
                r.name, r.family, faith(r.faith), per_node(r), bytes(r.bytes), if r.shared > 0 { bytes(r.shared) } else { "".into() }, bytes(r.state), r.build_ms,
                us(r.ns_mean), us(r.ns_p99), r.misses.map(|m| m.to_string()).unwrap_or_else(|| "n/a".into()), r.visits,
                pm2(r.agree.top1_pm, r.agree.top1_tie_pm), pm2(r.agree.ws_pm, r.agree.ws_tie_pm), pm2(r.agree.action_pm, r.agree.action_tie_pm), r.agree.regret_pm, r.agree.act_err_pm, if r.exact { "yes" } else { "" },
                if r.ticks < rows[0].ticks { format!("{} ticks. {}", r.ticks, r.note) } else { r.note.clone() });
        }
    }
    let _ = writeln!(s, "\n## Sharing a species base\n\nOne species base, many individuals. *Overlay*: link-level copy-on-write (reweights 6 B, added links 13 B, revalues 6 B, new nodes 3 B). *Page COW*: the same changes, copying every touched 64-node page whole. *Alone*: the individual as its own graph.\n");
    let _ = writeln!(s, "| Nodes | Minds | Individual differs by | Base (shared) | Overlay per extra mind (mean, max) | Page COW per mind | Alone per mind | Total, shared | Total, alone |\n|---|---|---|---|---|---|---|---|---|");
    for r in x.share {
        let _ = writeln!(s, "| {} | {} | {} | {} | {}, {} | {} | {} | {} | {} |", size(r.n), r.minds, r.delta, bytes(r.base_bytes), bytes(r.overlay_mean), bytes(r.overlay_max), bytes(r.page_cow_mean), bytes(r.independent_per_mind),
            bytes(r.base_bytes + r.overlay_mean * r.minds), bytes(r.independent_per_mind * r.minds));
    }
    let _ = writeln!(s, "\n## Fast weights and locality\n\n*Fast-weight links*: distinct links between co-active workspace nodes over the whole run (what a day's fast weights touch). Dense fast weights cost 2 B on every link. *Mean link gap*: mean |source − target| id distance.\n");
    let _ = writeln!(s, "| Nodes | Links | Fast-weight links touched | Dense (2 B/link) | Sparse (8 B/entry) | Mean link gap: generated | shuffled | shuffled → RCM |\n|---|---|---|---|---|---|---|---|");
    for (&(n, links, fw), &(_, a, b, c)) in x.fast.iter().zip(x.gaps) {
        let _ = writeln!(s, "| {} | {links} | {fw} | {} | {} | {a} | {b} | {c} |", size(n), bytes(links * 2), bytes(fw * 8));
    }
    let _ = writeln!(s, "\n## Bit vectors where they might win: recognition\n\n*Identify*: the percept's thing is found. *Category*: its category (of 8). *Food?*: food or not.\n");
    let _ = writeln!(s, "| Things | Method | Bytes | ns per query | Identify | Category | Food? |\n|---|---|---|---|---|---|---|");
    for (_, rs) in x.recog { for r in rs {
        let id = if r.method.contains("only") { "".into() } else { pm(r.id_pm) };
        let _ = writeln!(s, "| {} | {} | {} | {} | {} | {} | {} |", r.things, r.method, bytes(r.bytes), r.ns_query, id, pm(r.cat_pm), pm(r.food_pm));
    } }
    let _ = writeln!(s, "\n## Bit vectors where they might win: episode recall\n\n*Exact*: the recalled episode has the best activation-weighted overlap with the probe (ties count). *Quality*: the recalled episode's overlap as a share of the best.\n");
    let _ = writeln!(s, "| Mind | Episodes | Method | Bytes per episode | ns per query | Exact | Quality |\n|---|---|---|---|---|---|---|");
    for (n, rs) in x.episodes { for r in rs {
        let _ = writeln!(s, "| {} | {} | {} | {} | {} | {} | {} |", size(*n), r.episodes, r.method, r.bytes_per, r.ns_query, pm(r.exact_pm), pm(r.quality_pm));
    } }
    s
}
