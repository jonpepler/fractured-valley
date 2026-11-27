//! Every variant: same hash across passes, thread counts and instance orders; exact variants
//! equal the oracle; the redefined kernel's pruning changes nothing.
use fv_spread_kernels::{bench::*, delta::Delta, exact::Inst, harness::*, rand::Rng, redef, shared::Shared};
use mc::spread::Params;

#[test]
fn variants_stable_across_threads_and_order() {
    let p = Params::default();
    let sp = species(5000, 9);
    let set = make_set(&sp, 24, 60, 1, 1, 2);
    let oracle = fv_spread_kernels::bench::run_oracle(&sp, &set, &p, 1, 1, 24).hash;
    let mut rev: Vec<u32> = (0..24).rev().collect();
    let mut r = Rng(3);
    for i in (1..rev.len()).rev() { rev.swap(i, r.below(i as u64 + 1) as usize); }
    for v in VARIANTS {
        let h = run_variant(v, &set, &p, 1, None).hash;
        assert_eq!(h, run_variant(v, &set, &p, 1, None).hash, "{v} pass");
        for th in [2, 3, 4] { assert_eq!(h, run_variant(v, &set, &p, th, None).hash, "{v} threads {th}"); }
        assert_eq!(h, run_variant(v, &set, &p, 4, Some(&rev)).hash, "{v} order");
        if v.starts_with("exact") { assert_eq!(h, oracle, "{v} vs oracle"); }
    }
}

#[test]
fn redef_pruning_is_exact() {
    let p = Params::default();
    let run = |sh: &Shared, d: &Delta, tk: &[mc::gen::Tick], prune: bool| -> Vec<u64> {
        let mut sc = redef::Scratch::new(); let mut inst = Inst::new();
        tk.iter().map(|t| {
            if t.damp != 1000 { redef::damp(&mut inst, t.damp); }
            if prune { redef::step(sh, d, &mut inst, &t.mods, &t.inject, &p, u32::MAX, &mut sc).1 }
            else { redef::step_unpruned(sh, d, &mut inst, &t.mods, &t.inject, &p, &mut sc).1 }
        }).collect()
    };
    let s = species(20_000, 4);
    let sh = Shared::new(&s.base);
    for who in 0..8 {
        let d = Delta::new(&sh, &overlay(&s, who, 3));
        let tk = ticks(&s, who, 5, 300);
        assert_eq!(run(&sh, &d, &tk, true), run(&sh, &d, &tk, false), "who {who}");
    }
    for seed in 0..2000u64 {
        let (g, tk) = if seed % 2 == 0 { adversarial(seed) } else { overflowing(seed) };
        let sh = Shared::new(&g); let d = Delta::new(&sh, &Default::default());
        assert_eq!(run(&sh, &d, &tk, true), run(&sh, &d, &tk, false), "seed {seed}");
    }
}
