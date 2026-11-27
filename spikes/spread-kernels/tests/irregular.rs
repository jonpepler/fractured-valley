//! The wrap-around corners: states with negative x, ghost entries (x != 0 but not active) and
//! zero entries left in the active list, and instances that overflow i32 on their own.
//!
//! Crafted states respect what the oracle can reach: x never exceeds max_act (every add caps
//! it), and a node is listed at most once. A duplicate listing needs a wrapped increment that
//! cancels a node's x to exactly 0 followed by a positive one in the same step; the kernels
//! follow the model there, and that corner is not verified against the oracle.
use fv_spread_kernels::{delta::Delta, exact::{self, Inst}, fast, harness::*, model, rand::Rng, shared::Shared};
use mc::spread::{think, Activation, Params};

fn sorted(v: &[u32]) -> Vec<u32> { let mut v = v.to_vec(); v.sort(); v }

#[test]
fn model_matches_oracle_raw_when_overflowing() {
    let p = Params::default();
    for seed in 0..3000u64 {
        let (g, tk) = overflowing(seed);
        let mut a = Activation::new(g.len()); let mut b = model::Dense::new(g.len());
        for t in &tk {
            if t.damp != 1000 { a.damp(t.damp); b.damp(t.damp); }
            let o1 = think(&g, &mut a, &t.mods, &t.inject, &p);
            let o2 = b.think(&g, &t.mods, &t.inject, &p);
            assert!(o1 == o2 && a.act == b.act && sorted(&a.active) == sorted(&b.active), "seed {seed}");
        }
    }
}

#[test]
fn kernels_match_model_when_overflowing() {
    let p = Params::default();
    for seed in 0..3000u64 {
        let (g, tk) = overflowing(seed);
        let sh = Shared::new(&g); let d = Delta::new(&sh, &Default::default());
        let m = run_model(&g, &tk, &p);
        assert!(m == run_fast(&sh, &d, &tk, &p), "fast seed {seed}");
        assert!(m == run_exact(&sh, d.clone(), &tk, &p), "exact seed {seed}");
    }
}

#[test]
fn crafted_irregular_states() {
    let p = Params::default();
    let mut r = Rng(42);
    for seed in 0..3000u64 {
        let (g, tk) = if seed % 2 == 0 { adversarial(seed) } else { overflowing(seed) };
        let n = g.len();
        let mut act = vec![0i32; n]; let mut active = vec![];
        for _ in 0..r.range(0, 40) {
            let v = r.below(n as u64) as u32;
            act[v as usize] = [0, 1, 5, 6, 100, 3999, 4000, -7, -100000, -2_000_000_000][r.below(10) as usize];
            if r.below(4) != 0 && !active.contains(&v) { active.push(v); }
        }
        let mut a = Activation::new(n); a.act = act.clone(); a.active = active.clone();
        let mut b = model::Dense::new(n); b.act = act.clone(); b.active = active.clone();
        let sh = Shared::new(&g); let d = Delta::new(&sh, &Default::default());
        let mut fi = Inst::from_dense(&act, &active); let mut ei = fi.clone();
        let mut fsc = fast::Scratch::new(); let mut esc = exact::Scratch::new();
        for t in tk.iter().take(3) {
            if t.damp != 1000 { a.damp(t.damp); b.damp(t.damp); fi.damp(t.damp); ei.damp(t.damp); }
            let o1 = think(&g, &mut a, &t.mods, &t.inject, &p);
            let o2 = b.think(&g, &t.mods, &t.inject, &p);
            assert!(o1 == o2 && a.act == b.act && sorted(&a.active) == sorted(&b.active), "oracle/model seed {seed}");
            let h = model::hash(&b.state(), &o2);
            let (o3, h3) = fast::step(&sh, &d, &mut fi, &t.mods, &t.inject, &p, &mut fsc);
            assert!(o3 == o2 && h3 == h, "fast seed {seed}");
            let o4 = exact::step(&sh, &d, &mut ei, &t.mods, &t.inject, &p, &mut esc);
            assert!(o4 == o2 && model::hash(&ei.state(), &o4) == h, "exact seed {seed}");
            assert_eq!(Inst::from_dense(&b.act, &b.active).st, fi.st, "fast raw state seed {seed}");
        }
    }
}
