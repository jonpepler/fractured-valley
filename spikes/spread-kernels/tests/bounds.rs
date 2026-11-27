//! Property P (used by the pruned kernel): for fixed modifiers and gain class, the largest gain
//! over all values -1000..=1000 is reached at -1000, 0 or 1000. Checked over every value for
//! random and corner modifier sets. The pruned kernel is then checked end to end.
use fv_spread_kernels::{delta::Delta, harness::*, mc, rand::Rng, shared::{unkey, Shared}};
use mc::spread::{gain, Mods, Params};

fn check(m: &Mods) {
    for c in 0..32u32 {
        let g = |v: u32| { let (ty, tg, vv) = unkey(((c << 11) + v) as u16); gain(m, ty, tg, vv) };
        let hi = g(0).max(g(1000)).max(g(2000));
        for v in 0..2001 { assert!(g(v) <= hi, "{m:?} class {c} value {}", v as i32 - 1000); }
    }
}

#[test]
fn gain_max_at_three_points() {
    let mut r = Rng(31);
    for it in 0..1500 {
        let mut pick = |lo: i64, hi: i64| if it % 3 == 0 { [lo, hi, (lo + hi) / 2][r.below(3) as usize] } else { r.range(lo, hi) } as i32;
        check(&Mods { hunger: pick(0, 1000), arousal: pick(0, 1000), mood: pick(-1000, 1000), fatigue: pick(0, 1000), stress: pick(0, 1000) });
    }
}

#[test]
fn pruned_matches_oracle_and_model() {
    let p = Params::default();
    for &n in &[1000usize, 30_000] {
        let s = species(n, 2);
        let sh = Shared::new(&s.base);
        for who in 0..10u32 {
            let ov = overlay(&s, who, 5);
            let g = mc::gen::materialise(&s.base, &ov);
            let tk = ticks(&s, who, 13, 300);
            assert!(run_oracle(&g, &tk, &p) == run_pruned(&sh, &Delta::new(&sh, &ov), &tk, &p), "n {n} who {who}");
        }
    }
    for seed in 0..3000u64 {
        let (g, tk) = if seed % 2 == 0 { adversarial(seed) } else { overflowing(seed) };
        let sh = Shared::new(&g);
        assert!(run_model(&g, &tk, &p) == run_pruned(&sh, &Delta::new(&sh, &Default::default()), &tk, &p), "seed {seed}");
    }
}
