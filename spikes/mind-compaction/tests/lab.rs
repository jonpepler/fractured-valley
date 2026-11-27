//! The lab's guarantees: every variant is deterministic, the exact variants are exact, and the
//! faithful ones stay above an agreement floor. `cargo test --release`.

use fv_mind_compaction::bench::{self, Faith, SEED};
use fv_mind_compaction::forms::{Compact, Cow};
use fv_mind_compaction::gen;
use fv_mind_compaction::rng::StateHasher;
use fv_mind_compaction::spread::{Edge, Form};
use std::hash::{Hash, Hasher};

fn h<T: Hash>(x: &T) -> u64 { let mut s = StateHasher::new(1); x.hash(&mut s); s.finish() }

/// Floors for the variants meant to be faithful (per mille, tie-aware), set a little under the
/// lowest value measured at 1k–1M (`results/baseline.md`).
const TOP1_FLOOR: u32 = 950;
const WS_FLOOR: u32 = 950;
/// Action: the regret, not the raw agreement, since the decision rule is often a close call.
/// At 1k nodes ties alone cost up to ~150‰ (see the renumbered layouts), so the ceiling is loose.
const REGRET_CEILING: u32 = 60;
const ERR_CEILING: u32 = 30;

#[test]
fn harness_is_pinned() {
    // The generator, the overlay, the workload and the reference think, all pinned: if this
    // moves, the standing baseline moved, and results/ must be regenerated.
    let a = bench::lab(1000, SEED, 300);
    let b = bench::lab(1000, SEED, 300);
    assert_eq!(h(&a.g), h(&b.g));
    assert_eq!(h(&a.ticks), h(&b.ticks));
    assert_eq!(h(&a.ov), h(&b.ov));
    let mut v = bench::build(bench::Kind::Reference, &a);
    let r = bench::run(v.as_mut(), &a.ticks);
    println!("graph {:#x} ticks {:#x} overlay {:#x} reference run {:#x}", h(&a.g), h(&a.ticks), h(&a.ov), r.hash);
    assert_eq!(r.hash, PINNED_REFERENCE, "the reference run changed");
}
const PINNED_REFERENCE: u64 = 0xbdf9_7393_921b_b5da;

#[test]
fn every_variant_is_deterministic_and_faithful_ones_agree() {
    for n in [1000usize, 10_000] {
        let a = bench::lab(n, SEED, bench::TICKS);
        let b = bench::lab(n, SEED, bench::TICKS);
        let mut rv = bench::build(bench::Kind::Reference, &a);
        let reference = bench::run(rv.as_mut(), &a.ticks);
        drop(rv);
        for s in bench::specs().iter().filter(|s| n <= s.max_n) {
            // Two minds built separately from the same seed give the same run (and `run` itself
            // checks that its passes agree).
            let (row, ra) = bench::measure(&a, s, &reference);
            let mut vb = bench::build(s.kind, &b);
            let rb = bench::run(vb.as_mut(), &b.ticks[..s.ticks.min(b.ticks.len())]);
            assert_eq!(ra.hash, rb.hash, "{} at {n}: not deterministic", s.name);
            let g = row.agree;
            println!("{n:>6} {:<45} top1 {:>4} ws {:>4} action {:>4} (tie-aware {:>4} {:>4} {:>4}) regret {:>4} err {:>4} exact {}", s.name, g.top1_pm, g.ws_pm, g.action_pm, g.top1_tie_pm, g.ws_tie_pm, g.action_tie_pm, g.regret_pm, g.act_err_pm, row.exact);
            match s.faith {
                Faith::Exact => assert!(row.exact, "{} at {n}: meant to be exact", s.name),
                Faith::Floor => {
                    assert!(g.top1_tie_pm >= TOP1_FLOOR && g.ws_tie_pm >= WS_FLOOR && g.regret_pm <= REGRET_CEILING && g.act_err_pm <= ERR_CEILING,
                        "{} at {n}: below the agreement floor: {g:?}", s.name);
                }
                Faith::UpToTies => assert!(g.act_err_pm == 0 && g.ws_tie_pm == 1000 && g.top1_tie_pm == 1000, "{} at {n}: not identical up to ties: {g:?}", s.name),
                Faith::Explore => {}
            }
        }
    }
}

fn edges_of<F: Form>(f: &F, u: u32) -> Vec<(u32, i32, u8, i32)> {
    let mut e: Vec<Edge> = vec![];
    f.edges(u, &mut e);
    let mut v: Vec<_> = e.iter().map(|e| (e.to, e.w, e.ty, e.val)).collect();
    v.sort_unstable();
    v
}

#[test]
fn cow_reads_exactly_the_materialised_mind() {
    let (base, info) = gen::species(5000, SEED);
    let ov = gen::overlay(&base, &info, gen::OverlaySpec::DEFAULT.scaled(4, 1), SEED, 7);
    let g = gen::materialise(&base, &ov);
    for c in [Cow::new(&base, &ov), Cow::new(&base, &ov).dense()] {
        assert_eq!(c.len(), g.len());
        for u in 0..g.len() as u32 {
            assert_eq!(edges_of(&c, u), edges_of(&g, u), "node {u}");
            assert_eq!(c.val(u), g.val(u));
            assert_eq!(c.tags(u), g.tags(u));
        }
    }
}

#[test]
fn compact_keeps_every_target_type_and_valence_sign() {
    let (g, _) = gen::species(5000, SEED);
    let c = Compact::compile(&g);
    let key = |v: Vec<(u32, i32, u8, i32)>, quantised: bool| -> Vec<(u32, u8, bool)> {
        let mut k: Vec<_> = v.into_iter().map(|e| (e.0, e.2, if quantised { e.3 != 0 } else { e.3 / 8 != 0 })).collect();
        k.sort_unstable();
        k
    };
    for u in 0..g.len() as u32 {
        assert_eq!(key(edges_of(&g, u), false), key(edges_of(&c, u), true), "node {u}");
    }
}
