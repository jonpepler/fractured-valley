//! Think v2 against its spec (`SPEC-think.md`), on hand-made graphs: each number below is worked
//! out by hand from the spec's formulas, then checked. Then properties: the early stop changes
//! nothing; the compiled cache (runtime form) equals sorting on the spot (expanded form), through
//! learning; ties go by stable key, so renumbering changes nothing; the caps saturate.
//! `cargo test --release --test think_v2 -- --nocapture`

use fv_mind_lab::graph::{lt, tag, Builder};
use fv_mind_lab::rng::hash;
use fv_mind_lab::store::{Base, Expanded, Runtime, Store};
use fv_mind_lab::think::{Activation, Form, Mods, Params};
use fv_mind_lab::think2::{self, Knots, A_CAP, F_CAP, INC_CAP, SAT_FACTOR, SAT_INC, SAT_INJECT};
use std::sync::Arc;

fn rt(b: Builder) -> Runtime { Runtime::new(Arc::new(Base::compile(b))) }
fn act(st: &Activation, n: u32) -> i32 { st.act[n as usize] }
fn think(f: &Runtime, st: &mut Activation, m: &Mods, inj: &[(u32, i32)], p: &Params) -> think2::Out {
    let v = f.sorted_view();
    think2::think_in(f, v.as_ref(), st, m, inj, p, None)
}

/// A sends to B (600) and C (400), assoc links, calm modulators.
fn fan() -> (Runtime, [u32; 3]) {
    let mut b = Builder::new(7);
    let a = b.node("a", 0, 0, None); let bb = b.node("b", 0, 0, None); let c = b.node("c", 0, 0, None);
    b.link(a, bb, lt::ASSOC, 600); b.link(a, c, lt::ASSOC, 400);
    (rt(b), [a, bb, c])
}

#[test]
fn one_hop_by_hand() {
    let (f, [a, b, c]) = fan();
    let p = Params::default();
    let mut st = Activation::new(f.len());
    // Knots: assoc gain 900, nothing modulating: g0 = 900; F = 900 × 1000/1000 × 550/1000 = 495.
    let k = Knots::new(&Mods::default(), p.hop_decay);
    assert_eq!(k.f[lt::ASSOC as usize * 8], [495, 495, 495]);
    let o = think(&f, &mut st, &Mods::default(), &[(a, 1000)], &p);
    // a: injected 1000. b: q = 1000 × 600 / 1000 = 600, flow 600 × 495 / 1000 = 297. c: 400 → 198.
    assert_eq!((act(&st, a), act(&st, b), act(&st, c)), (1000, 297, 198));
    assert_eq!(o.ws, vec![a, b, c]);
    // Visits: a's 2 links in hop 1; b and c have none (hop 2); then the frontier is empty.
    assert_eq!((o.visits, o.hops_run), (2, 2));
    assert_eq!(st.active, vec![a, b, c]);
    // The next think: persist 500 (1000 → 500, 297 → 148, 198 → 99), carry 300 of that spreads:
    // a seeds 150, b 44, c 29. a → b: 150 × 600 / 1000 = 90 × 495 / 1000 = 44; a → c: 60 → 29.
    let o = think(&f, &mut st, &Mods::default(), &[], &p);
    assert_eq!((act(&st, a), act(&st, b), act(&st, c)), (500, 148 + 44, 99 + 29));
    assert_eq!(o.ws, vec![a, b, c]);
}

#[test]
fn gain_knots_and_valence_by_hand() {
    // Kind links (gain 1000, not damped by arousal) into a target felt at −500 under arousal 500.
    let m = Mods { arousal: 500, ..Mods::default() };
    let k = Knots::new(&m, 550);
    let c = lt::KIND as usize * 8;
    // g0 = 1000; M− = 1000 + 2 × 500 = 2000: F− = 1000 × 2000/1000 × 550/1000 = 1100; F0 = 550; M+ = 1000.
    assert_eq!(k.f[c], [1100, 550, 550]);
    assert_eq!(k.factor(c, -500), 550 + 550 * 500 / 1000);
    assert_eq!(k.factor(c, 1000), 550);
    // Assoc under the same arousal: 900 × (1000 − 250)/1000 = 675; F0 = 675 × 550/1000 = 371.
    assert_eq!(k.f[lt::ASSOC as usize * 8][1], 371);
    // Food-tagged target, hunger 1000: x = 1000 + 1000 × 1000/1000 = 2000; F0 = 1100.
    let k = Knots::new(&Mods { hunger: 1000, ..Mods::default() }, 550);
    assert_eq!(k.f[c + 1][1], 1100);
    // Low mood −1000: M+ = 1000 + 0 − 1000 = 0, so a liked target gets nothing; M− = 3000.
    let k = Knots::new(&Mods { mood: -1000, ..Mods::default() }, 550);
    assert_eq!(k.f[c], [1650, 550, 0]);
    assert_eq!(k.factor(c, 500), 275);
    // Truncation towards zero on the falling side: 550 + (0 − 550) × 333 / 1000 = 550 − 183.
    assert_eq!(k.factor(c, 333), 367);
    // The per-type bound is the largest knot over its classes.
    assert_eq!(k.bt[lt::KIND as usize], k.f[c..c + 8].iter().flat_map(|x| x.iter().copied()).max().unwrap());

    // And through a think: a → t (kind, 1000) where t feels −500, under arousal 500.
    let mut b = Builder::new(1);
    let a = b.node("a", 0, 0, None); let t = b.node("t", -500, 0, None);
    b.link(a, t, lt::KIND, 1000);
    let f = rt(b);
    let mut st = Activation::new(f.len());
    think(&f, &mut st, &m, &[(a, 800)], &Params::default());
    // q = 800 × 1000 / 1000 = 800; factor 825; flow 800 × 825 / 1000 = 660.
    assert_eq!(act(&st, t), 660);
}

#[test]
fn weight_sum_floor_and_focus() {
    // Fan-out divides by the weight sum, at least 1000: two links of 200 send 20% each, not 50%.
    let mut b = Builder::new(2);
    let a = b.node("a", 0, 0, None); let x = b.node("x", 0, 0, None); let y = b.node("y", 0, 0, None);
    b.link(a, x, lt::KIND, 200); b.link(a, y, lt::KIND, 200);
    let f = rt(b);
    let mut st = Activation::new(f.len());
    think(&f, &mut st, &Mods::default(), &[(a, 1000)], &Params::default());
    // q = 1000 × 200 / 1000 = 200; × 550 / 1000 = 110.
    assert_eq!((act(&st, x), act(&st, y)), (110, 110));
    // Focus on intent x (focus 1000): flows into x and what x links to are doubled.
    let mut st = Activation::new(f.len());
    think(&f, &mut st, &Mods { focus: 1000, intent: x, ..Mods::default() }, &[(a, 1000)], &Params::default());
    assert_eq!((act(&st, x), act(&st, y)), (220, 110));
}

#[test]
fn budget_cap_and_ties_by_key() {
    // A hub to 12 leaves of equal weight: equal flows, so the cap keeps the leaves with the
    // smallest stable keys.
    let mut b = Builder::new(3);
    let h = b.node("hub", 0, 0, None);
    let leaves: Vec<u32> = (0..12).map(|i| b.node(&format!("l{i}"), 0, 0, None)).collect();
    for &l in &leaves { b.link(h, l, lt::KIND, 500); }
    let f = rt(b);
    let p = Params { active_cap: 5, ws_k: 3, budget: 2000, ..Params::default() };
    let mut st = Activation::new(f.len());
    let o = think(&f, &mut st, &Mods::default(), &[(h, 4000)], &p);
    // Each leaf: q = 4000 × 500 / 6000 = 333; × 550 / 1000 = 183. Total 4000 + 12 × 183 = 6196 > 2000:
    // hub 4000 × 2000 / 6196 = 1291, leaves 183 × 2000 / 6196 = 59.
    assert_eq!(act(&st, h), 1291);
    let mut by_key = leaves.clone(); by_key.sort_by_key(|&l| (f.key(l), l));
    let kept: Vec<u32> = { let mut v = by_key[..4].to_vec(); v.push(h); v.sort(); v };
    assert_eq!(st.active, kept, "the cap keeps the hub and the 4 leaves with the smallest keys");
    for &l in &by_key[..4] { assert_eq!(act(&st, l), 59); }
    for &l in &by_key[4..] { assert_eq!(act(&st, l), 0); }
    assert_eq!(o.ws, vec![h, by_key[0], by_key[1]]);
}

#[test]
fn caps_saturate_and_say_so() {
    // An injection above A_CAP is clamped (and flagged); activation stops at max_act.
    let (f, [a, b, _]) = fan();
    let mut st = Activation::new(f.len());
    let o = think(&f, &mut st, &Mods::default(), &[(a, 50_000)], &Params { budget: 131_071, ..Params::default() });
    assert_ne!(o.flags & SAT_INJECT, 0);
    assert_eq!(act(&st, a), 4000);
    // The sender sends A_CAP, not 50,000: q = 16383 × 600 / 1000 = 9829; × 495 / 1000 = 4865 → max 4000.
    assert_eq!(act(&st, b), 4000);

    // Many strong senders into one node: the hop's sum saturates at INC_CAP; the factor at F_CAP.
    let mut bd = Builder::new(4);
    let t = bd.node("t", -1000, tag::FOOD | tag::REST | tag::ANGER, None);
    let s: Vec<u32> = (0..900).map(|i| bd.node(&format!("s{i}"), 0, 0, None)).collect();
    for &x in &s { bd.link(x, t, lt::WANTS, 1000); }
    let f = rt(bd);
    let m = Mods { arousal: 1000, mood: -1000, hunger: 1000, fatigue: 1000, stress: 1000, focus: 1000, intent: t, ..Mods::default() };
    let p = Params { frontier: 1000, hop_decay: 1000, max_act: A_CAP, budget: 131_071, active_cap: 2000, ..Params::default() };
    let k = Knots::new(&m, p.hop_decay);
    assert!(k.f[lt::WANTS as usize * 8 + 7][0] > 0 && k.f[lt::WANTS as usize * 8 + 7][0] <= F_CAP);
    let inj: Vec<(u32, i32)> = s.iter().map(|&x| (x, A_CAP)).collect();
    let mut st = Activation::new(f.len());
    let o = think(&f, &mut st, &m, &inj, &p);
    // F_CAP cannot bind: the largest knot (wants, every tag, every modulator at its extreme) is
    // 16000 × 667/1000 × 5000/1000 = 53,360, and focus and courage at most double it twice.
    assert_eq!(k.f[lt::WANTS as usize * 8 + 7][0], 53_360);
    assert!(53_360 * 4 <= F_CAP);
    assert_eq!(o.flags & SAT_FACTOR, 0);
    assert_ne!(o.flags & SAT_INC, 0, "900 × 4.29M > 2^30");
    let _ = INC_CAP;
    println!("knot for the strongest class: {:?}; flags {:b}", k.f[lt::WANTS as usize * 8 + 7], o.flags);
}

/// A random graph of `n` nodes with about `deg` links each, random weights, valences and tags.
fn random(seed: u64, n: u32, deg: u32) -> Builder {
    let mut b = Builder::new(seed);
    for i in 0..n {
        let r = hash(seed, i as u64, 1, 0);
        b.node(&format!("n{i}"), (r % 2001) as i32 - 1000, ((r >> 12) % 256) as u8 & !tag::ANCHORED, None);
    }
    for i in 0..n {
        for j in 0..deg {
            let r = hash(seed, i as u64, j as u64, 2);
            let t = (r % n as u64) as u32;
            b.link_v(i, t, ((r >> 20) % lt::COUNT as u64) as u8, ((r >> 32) % 1001) as i32, ((r >> 44) % 2001) as i32 - 1000);
        }
    }
    b
}

fn mods(seed: u64, i: u64, n: u32) -> Mods {
    let r = |k| (hash(seed, i, k, 9) % 1001) as i32;
    Mods { hunger: r(1), arousal: r(2), mood: r(3) * 2 - 1000, fatigue: r(4), stress: r(5), focus: r(6) / 2, intent: (hash(seed, i, 7, 9) % n as u64) as u32, hunger2: 0, courage: r(8) / 3, ally: (hash(seed, i, 10, 9) % n as u64) as u32 }
}

#[test]
fn early_stop_and_cache_change_nothing_through_learning() {
    for seed in 0..6u64 {
        let n = 300;
        let mut r = rt(random(seed, n, 12));
        let mut e: Expanded = r.expand();
        let p = Params { frontier: 32, ..Params::default() };
        let (mut sa, mut sb, mut sc) = (Activation::new(n as usize), Activation::new(n as usize), Activation::new(n as usize));
        for i in 0..200u64 {
            let m = mods(seed, i, n);
            let inj: Vec<(u32, i32)> = (0..3).map(|k| ((hash(seed, i, k, 3) % n as u64) as u32, (hash(seed, i, k, 4) % 1500) as i32)).collect();
            let a = think(&r, &mut sa, &m, &inj, &p);
            let b = think2::think_in(&e, None, &mut sb, &m, &inj, &p, None);
            think2::PRUNE.store(false, std::sync::atomic::Ordering::Relaxed);
            let c = think(&r, &mut sc, &m, &inj, &p);
            think2::PRUNE.store(true, std::sync::atomic::Ordering::Relaxed);
            assert_eq!(a, b, "seed {seed} think {i}: cached runtime form against the expanded form");
            assert_eq!(a, c, "seed {seed} think {i}: early stop on against off");
            assert_eq!((&sa.active, &sb.active), (&sc.active, &sc.active));
            for &x in &sa.active { assert_eq!(sa.act[x as usize], sb.act[x as usize]); assert_eq!(sa.act[x as usize], sc.act[x as usize]); }
            // Learn on both forms the same way: fast weights, reweights, link valences, a new node
            // with links, and removing one of the mind's own links.
            let u = (hash(seed, i, 5, 5) % n as u64) as u32;
            let ls = r.links_vec(u);
            if !ls.is_empty() {
                let l = ls[(i as usize) % ls.len()];
                let fw = (hash(seed, i, 6, 6) % 600) as i16 - 200;
                r.set_fast(l.key, fw); e.set_fast(l.key, fw);
                if i % 3 == 0 { r.set_slow(l.key, 300); e.set_slow(l.key, 300); }
                if i % 5 == 0 { r.set_lval(l.key, -400); e.set_lval(l.key, -400); }
            }
            if i % 4 == 0 { r.map_fast(&mut |f| f - f / 4); e.map_fast(&mut |f| f - f / 4); }
            if i % 17 == 0 {
                let x = r.add_node("new", 100, tag::FOOD, None, seed); let y = e.add_node("new", 100, tag::FOOD, None, seed);
                assert_eq!(x, y);
                r.add_link(u, x, lt::ASSOC, 700, 0); e.add_link(u, x, lt::ASSOC, 700, 0);
                let k = r.add_link(x, u, lt::KIND, 500, 50); e.add_link(x, u, lt::KIND, 500, 50);
                r.add_link(x, (u + 1) % n, lt::ASSOC, 400, 0); e.add_link(x, (u + 1) % n, lt::ASSOC, 400, 0);
                if i % 34 == 0 { r.remove_link(k); e.remove_link(k); }
                for s in [&mut sa, &mut sb, &mut sc] { s.ensure(r.len()); }
            }
            if i % 7 == 0 { r.set_val(u, 900); e.set_val(u, 900); }
        }
        let c = r.cache.0.lock().unwrap();
        println!("seed {seed}: {} nodes patched of {}, {} recompiled over {} refreshes", c.patched(), r.len(), c.recompiled, c.refreshes);
    }
}

#[test]
fn renumbering_changes_nothing() {
    // The same graph with its nodes in reverse order (and so different ids): the same thoughts, by
    // name, because ties go by stable key.
    let make = |rev: bool| {
        let mut b = Builder::new(5);
        let names: Vec<String> = (0..40).map(|i| format!("n{i}")).collect();
        let order: Vec<usize> = if rev { (0..40).rev().collect() } else { (0..40).collect() };
        let mut id = vec![0u32; 40];
        for &i in &order { id[i] = b.node(&names[i], 0, 0, None); }
        // Fix each node's key to its name's, so the two numberings share keys.
        for i in 0..40 { b.keys[id[i] as usize] = hash(5, i as u64, 0, 0) as u32; }
        for i in 0..40u64 { for j in 0..4u64 { let t = (hash(5, i, j, 1) % 40) as usize; b.link(id[i as usize], id[t], lt::ASSOC, 400); } }
        (rt(b), id)
    };
    let ((fa, ia), (fb, ib)) = (make(false), make(true));
    let p = Params { frontier: 6, active_cap: 10, ..Params::default() };
    let (mut sa, mut sb) = (Activation::new(40), Activation::new(40));
    for t in 0..30u64 {
        let i = (hash(9, t, 0, 0) % 40) as usize;
        let oa = think(&fa, &mut sa, &Mods::default(), &[(ia[i], 900)], &p);
        let ob = think(&fb, &mut sb, &Mods::default(), &[(ib[i], 900)], &p);
        let name = |f: &Runtime, v: &[u32]| v.iter().map(|&n| f.name(n).to_string()).collect::<Vec<_>>();
        assert_eq!(name(&fa, &oa.ws), name(&fb, &ob.ws), "think {t}");
        for k in 0..40 { assert_eq!(sa.act[ia[k] as usize], sb.act[ib[k] as usize]); }
    }
}
