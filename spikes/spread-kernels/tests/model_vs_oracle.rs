use fv_spread_kernels::{harness::*, mc};
use mc::spread::Params;

#[test]
fn model_matches_oracle_on_species() {
    let p = Params::default();
    for &n in &[1000usize, 10_000] {
        let s = species(n, 5);
        for who in 0..12u32 {
            let g = mc::gen::materialise(&s.base, &overlay(&s, who, 9));
            let tk = ticks(&s, who, 21, 300);
            assert_eq!(run_oracle(&g, &tk, &p), run_model(&g, &tk, &p), "n {n} who {who}");
        }
    }
}

#[test]
fn model_matches_oracle_adversarial() {
    let p = Params::default();
    let k: u64 = std::env::var("ADV").ok().and_then(|s| s.parse().ok()).unwrap_or(3000);
    for seed in 0..k {
        let (g, tk) = adversarial(seed);
        assert!(run_oracle(&g, &tk, &p) == run_model(&g, &tk, &p), "seed {seed}");
    }
}
