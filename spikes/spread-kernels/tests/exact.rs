use fv_spread_kernels::{delta::Delta, harness::*, mc, shared::Shared};
use mc::spread::Params;

#[test]
fn exact_matches_oracle_on_species() {
    let p = Params::default();
    for &n in &[1000usize, 30_000] {
        let s = species(n, 5);
        let sh = Shared::new(&s.base);
        for who in 0..10u32 {
            let ov = overlay(&s, who, 9);
            let g = mc::gen::materialise(&s.base, &ov);
            let tk = ticks(&s, who, 21, 300);
            assert!(run_oracle(&g, &tk, &p) == run_exact(&sh, Delta::new(&sh, &ov), &tk, &p), "n {n} who {who}");
        }
    }
}

#[test]
fn exact_matches_model_adversarial() {
    let p = Params::default();
    let k: u64 = std::env::var("ADV").ok().and_then(|s| s.parse().ok()).unwrap_or(3000);
    for seed in 0..k {
        let (g, tk) = adversarial(seed);
        let sh = Shared::new(&g);
        let d = Delta::new(&sh, &Default::default());
        assert!(run_model(&g, &tk, &p) == run_exact(&sh, d, &tk, &p), "seed {seed}");
    }
}
