use fv_spread_kernels::{delta::*, harness::*, mc, shared::*};
use mc::spread::{Edge, Form, gain, Mods};
use fv_spread_kernels::rand::Rng;

/// The compiled delta gives, for every node, exactly the materialised graph's edges (as
/// (target, weight, gain key) multisets) and denominators.
#[test]
fn delta_equals_materialised() {
    for &n in &[1000usize, 20_000] {
        let s = species(n, 3);
        let sh = Shared::new(&s.base);
        for who in 0..6 {
            let ov = overlay(&s, who, 77);
            let g = mc::gen::materialise(&s.base, &ov);
            let d = Delta::new(&sh, &ov);
            let v = View { sh: &sh, d: &d };
            assert_eq!(g.len(), v.len());
            let mut a = vec![];
            for u in 0..g.len() as u32 {
                assert_eq!(g.val(u), v.val(u)); assert_eq!(g.tags(u) & !64, v.tags(u));
                a.clear(); g.edges(u, &mut a);
                let mut ka: Vec<_> = a.iter().map(|e: &Edge| (e.to, e.w as i16, gkey(e.ty, g.tags(e.to), g.val(e.to) + e.val))).collect(); ka.sort();
                let mut kb = vec![]; let mut den = 0;
                d.for_edges(&sh, u, |dn, to, w, k| { den = dn; if w > 0 { kb.push((to, w, k)); } });
                kb.sort();
                assert_eq!(ka, kb, "n {n} who {who} u {u}");
                if !kb.is_empty() { assert_eq!(den, ka.iter().map(|e| e.1 as i32).sum::<i32>().max(1000)); }
            }
        }
    }
}

#[test]
fn gain_depends_only_on_key() {
    let mut r = Rng(99);
    for _ in 0..3000 {
        let m = Mods { hunger: r.range(0, 1000) as i32, arousal: r.range(0, 1000) as i32, mood: r.range(-1000, 1000) as i32, fatigue: r.range(0, 1000) as i32, stress: r.range(0, 1000) as i32 };
        for _ in 0..300 {
            let (ty, tg, v) = (r.below(8) as u8, r.below(256) as u8, r.range(-2500, 2500) as i32);
            let (a, b, c) = unkey(gkey(ty, tg, v));
            assert_eq!(gain(&m, ty, tg, v), gain(&m, a, b, c));
            assert!(gain(&m, ty, tg, v) >= 0);
        }
    }
}

/// Every (type, tags, value) for 40 random modifier sets: about 350M oracle calls.
#[test]
#[ignore]
fn gain_depends_only_on_key_exhaustive() {
    let mut r = Rng(1234);
    for _ in 0..40 {
        let m = Mods { hunger: r.range(0, 1000) as i32, arousal: r.range(0, 1000) as i32, mood: r.range(-1000, 1000) as i32, fatigue: r.range(0, 1000) as i32, stress: r.range(0, 1000) as i32 };
        for ty in 0..8u8 { for tg in 0..=255u8 { for v in -2100..=2100 {
            let (a, b, c) = unkey(gkey(ty, tg, v));
            assert_eq!(gain(&m, ty, tg, v), gain(&m, a, b, c));
        } } }
    }
}
