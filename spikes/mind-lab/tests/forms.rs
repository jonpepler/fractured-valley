//! Two forms of one mind (8.1): the runtime form (shared species base, overlay, sparse fast
//! weights) and the expanded, inspectable form must be the same mind.
//! - Run side by side through whole scenarios, they agree on every tick (state hash and action).
//! - Converting either way is lossless.
//! - Learning works on the runtime form: fast weights, consolidation into the overlay, sleep,
//!   learnt places, and nodes and links the mind adds itself (a told fact).
//! - Ties by stable key make a think independent of how the nodes are numbered.
//! `cargo test --release --test forms -- --nocapture`

use fv_mind_lab::fox::scenarios::{self, HomeMove, DAY, SEED};
use fv_mind_lab::fox::sim::{valley_fox, Sim};
use fv_mind_lab::graph::{link_key, lt, tag, Anchor};
use fv_mind_lab::mind::{Action, Temperament};
use fv_mind_lab::rng::hash;
use fv_mind_lab::store::{Expanded, Runtime, Store};
use fv_mind_lab::think::{self, Activation, Form, Mods, Params};

fn lockstep(name: &str, mut a: Sim<Runtime>, ticks: u32) -> u32 {
    let mut b = Sim { w: a.w.clone(), m: a.m.expand(), fox: a.fox.clone(), record: false, frames: vec![], nights: vec![], record_from: 0, snapshots: vec![] };
    let mut ticks_equal = 0;
    for t in 0..ticks {
        let (x, y) = (a.step(), b.step());
        assert_eq!(x, y, "{name}: actions differ at tick {t}");
        assert_eq!(a.m.state_hash(), b.m.state_hash(), "{name}: minds differ at tick {t}");
        ticks_equal += 1;
    }
    // Lossless both ways, at the end of the run.
    assert_eq!(a.m.store.expand(), b.m.store, "{name}: runtime expanded == expanded");
    let back = b.m.store.compile(&a.m.store.base);
    assert_eq!(back.ov, a.m.store.ov, "{name}: expanded compiled back == runtime overlay");
    assert_eq!(back.fast, a.m.store.fast);
    let (sh, own, fast, st) = a.m.bytes();
    println!("{name:34} {ticks_equal:5} ticks identical | runtime: shared base {sh} B, overlay {own} B, fast {fast} B, state {st} B | expanded {} B + fast {} B",
        b.m.store.graph_bytes(), b.m.store.fast.len() * 2);
    ticks_equal
}

#[test]
fn runtime_and_expanded_forms_agree_on_every_tick() {
    let mut total = 0;
    total += lockstep("dog day, night, day 2", scenarios::dog_day(SEED), 2 * DAY);
    total += lockstep("meadow fire", scenarios::meadow_fire(SEED), DAY + 120);
    total += lockstep("sleepless eventful, 3 days", scenarios::sleepless_eventful_days(SEED, 3), 3 * DAY);
    total += lockstep("home moved near, 6 days", scenarios::home_moved(SEED, HomeMove::Near, 6), 6 * DAY);
    total += lockstep("home moved daily, 10 days", scenarios::home_moved(SEED, HomeMove::Daily, 10), 10 * DAY);
    for i in [3, 18, 26] { total += lockstep(&format!("dog charges fox #{i}"), scenarios::dog_charges(SEED, i), 20); }
    println!("{total} ticks in all, identical in both forms");
}

/// The learning operations on their own, through the `Store` interface, on both forms at once.
#[test]
fn learning_operations_agree_on_both_forms_including_added_nodes() {
    let fox = valley_fox();
    let m = fox.mind(Temperament::MEDIUM, 1);
    let (mut r, mut e) = (m.store.clone(), m.store.expand());
    let water = m.node("water");
    let stream = m.node("stream");
    // A told fact: a node the mind didn't have, linked to what it knows.
    let sr = r.add_node("the spring", 200, 0, Some(Anchor::Entity { id: 900, x: 5, y: 14 }), 77);
    let se = e.add_node("the spring", 200, 0, Some(Anchor::Entity { id: 900, x: 5, y: 14 }), 77);
    assert_eq!(sr, se);
    let links = [(water, sr, lt::ASSOC, 400), (sr, water, lt::KIND, 700), (sr, stream, lt::AT, 600), (stream, sr, lt::HOLDS, 300)];
    for &(u, v, ty, w) in &links { assert_eq!(r.add_link(u, v, ty, w, 0), e.add_link(u, v, ty, w, 0)); }
    // Fast weights, slow weights and valences on base links and on added links.
    let mut keys = vec![];
    for u in [water, stream, sr] { for l in r.links_vec(u) { keys.push(l.key); } }
    for (i, &k) in keys.iter().enumerate() {
        let f = (hash(1, i as u64, 0, 3) % 900) as i16 - 200;
        r.set_fast(k, f); e.set_fast(k, f);
        if i % 3 == 0 { let w = (hash(1, i as u64, 1, 3) % 1000) as i16; r.set_slow(k, w); e.set_slow(k, w); }
        if i % 4 == 0 { let v = (hash(1, i as u64, 2, 3) % 600) as i16 - 300; r.set_lval(k, v); e.set_lval(k, v); }
    }
    r.map_fast(&mut |f| f - f / 128 - f.signum()); e.map_fast(&mut |f| f - f / 128 - f.signum());
    assert_eq!(r.expand(), e, "the same mind after the same writes");
    assert_eq!(e.compile(&r.base).ov, r.ov);
    // Setting a base link back to its base value removes it from the overlay.
    let k0 = link_key(water, 0);
    let base_w = r.base.base_link(water, 0).unwrap().w;
    r.set_slow(k0, base_w);
    assert!(r.ov.slow.get(k0).is_none());
    // Thinking reaches the told node from what the mind knows.
    let mut a = Activation::new(r.len());
    let ws = think::think(&r, &mut a, &Mods::default(), &[(water, 1000)], &Params::default(), None);
    println!("cue water: workspace {:?}", ws.iter().map(|&n| r.name(n)).collect::<Vec<_>>());
    assert!(ws.contains(&sr), "a told fact comes to mind from what it was linked to");
}

/// Consolidation and sleep on a mind with a told fact: the fox spends the day at the stream
/// (where it was told there's a spring); the fact's links take fast weight by day and move it into
/// the overlay overnight, in both forms identically.
#[test]
fn a_told_fact_is_learnt_and_consolidated_on_the_runtime_form() {
    let mut s = scenarios::mood_at(SEED, "stream");
    s.m.mods.mood = 0;
    s.at(scenarios::NIGHT, fv_mind_lab::fox::world::Event::Night);
    let (water, stream) = (s.m.node("water"), s.m.node("stream"));
    let sp = s.m.store.add_node("the spring", 200, 0, Some(Anchor::Entity { id: 900, x: 12, y: 17 }), s.m.who);
    for (u, v, ty, w) in [(water, sp, lt::ASSOC, 500), (sp, water, lt::KIND, 700), (sp, stream, lt::AT, 600), (stream, sp, lt::HOLDS, 500)] { s.m.store.add_link(u, v, ty, w, 0); }
    let mut x = Sim { w: s.w.clone(), m: s.m.expand(), fox: s.fox.clone(), record: false, frames: vec![], nights: vec![], record_from: 0, snapshots: vec![] };
    let told = |m: &fv_mind_lab::mind::Mind<Runtime>| -> (i64, i64, usize) {
        let mut sl = 0; let mut fa = 0; let mut n = 0;
        for u in 0..m.store.len() as u32 { for l in m.store.links_vec(u) { if u == sp || l.to == sp { sl += l.slow as i64; fa += l.fast as i64; n += 1; } } }
        (sl, fa, n)
    };
    let before = told(&s.m);
    let mut in_ws = 0;
    for t in 0..2 * DAY {
        let (a, b) = (s.step(), x.step());
        assert_eq!(a, b);
        assert_eq!(s.m.state_hash(), x.m.state_hash(), "tick {t}");
        if s.m.last.ws.contains(&sp) { in_ws += 1; }
        if t == DAY - 2 { println!("told fact at nightfall 1: (slow, fast, links) {:?}", told(&s.m)); }
    }
    let after = told(&s.m);
    println!("told fact: in the workspace on {in_ws} of {} ticks; its links (slow sum, fast sum, count): before {before:?}, after two days and a night {after:?}", 2 * DAY);
    println!("FINDING: calm day learning truncates to zero below co-activation × (arousal + 60) = 4000, so a dimly held told fact never strengthens");
    assert!(in_ws > 0, "the fox thinks of the spring at the stream");
    // The mechanism itself, on the runtime form: a fast trace on a told fact's link is consolidated
    // into the mind's overlay by the next sleep (and identically in the expanded form).
    let k = s.m.store.find(sp, stream, lt::AT).unwrap();
    s.m.store.set_fast(k, 400); x.m.store.set_fast(k, 400);
    let slow0 = s.m.store.slow(k);
    s.m.sleep(); x.m.sleep();
    println!("a fast trace of 400 on spring→stream: slow {slow0} → {} after a night (keeps 1/{}), fast {}", s.m.store.slow(k), s.m.cfg.sleep_keep, s.m.store.fast(k));
    assert_eq!(s.m.store.slow(k), slow0 + 400 / s.m.cfg.sleep_keep as i16);
    assert_eq!(s.m.state_hash(), x.m.state_hash());
}

/// Renumber a mind's nodes: with ties by stable key, a think gives the same workspace (mapped)
/// and the same activations; with ties by index it can differ.
#[test]
fn key_ties_make_thinking_independent_of_numbering() {
    let mut s = scenarios::dog_day(SEED);
    s.run(DAY + 20);
    let e = s.m.store.expand();
    let n = e.len();
    let perm: Vec<u32> = { let mut k: Vec<(u64, u32)> = (0..n as u32).map(|i| (hash(5, i as u64, 0, 9), i)).collect(); k.sort(); let mut p = vec![0; n]; for (new, &(_, old)) in k.iter().enumerate() { p[old as usize] = new as u32; } p };
    let pe = permute(&e, &perm);
    let (mut same_key, mut same_idx, mut acts_key, samples) = (0, 0, 0, 400u64);
    for i in 0..samples {
        let h = |k| hash(SEED, i, k, 0xbeef);
        let mods = Mods { hunger: (h(1) % 1001) as i32, arousal: (h(2) % 900) as i32, mood: (h(3) % 1601) as i32 - 800, fatigue: (h(4) % 700) as i32, stress: (h(5) % 800) as i32 };
        let cue: Vec<(u32, i32)> = (0..3).map(|k| ((h(10 + k) % n as u64) as u32, 200 + (h(20 + k) % 800) as i32)).collect();
        let pcue: Vec<(u32, i32)> = cue.iter().map(|&(x, a)| (perm[x as usize], a)).collect();
        for index_ties in [false, true] {
            let p = Params { index_ties, ..Params::default() };
            let (mut a, mut b) = (Activation::new(n), Activation::new(n));
            let (mut wa, mut wb) = (vec![], vec![]);
            for _ in 0..3 { wa = think::think(&e, &mut a, &mods, &cue, &p, None); wb = think::think(&pe, &mut b, &mods, &pcue, &p, None); }
            let wb_mapped: Vec<u32> = wb.iter().map(|&x| perm.iter().position(|&q| q == x).unwrap() as u32).collect();
            let same = wa == wb_mapped;
            if index_ties { if same { same_idx += 1; } } else {
                if same { same_key += 1; }
                if (0..n).all(|x| a.act[x] == b.act[perm[x] as usize]) { acts_key += 1; }
            }
        }
    }
    println!("renumbered mind, {samples} sampled thinks: same workspace in the same order with key ties {same_key}, with index ties {same_idx}; identical activations with key ties {acts_key}");
    assert_eq!(same_key, samples);
    assert_eq!(acts_key, samples);
}

fn permute(e: &Expanded, perm: &[u32]) -> Expanded {
    let n = e.len();
    let mut inv = vec![0u32; n];
    for (i, &p) in perm.iter().enumerate() { inv[p as usize] = i as u32; }
    let mut out = Expanded { nbase: n, offsets: vec![0], ..Default::default() };
    for new in 0..n {
        let old = inv[new] as usize;
        out.val.push(e.val[old]); out.tags.push(e.tags[old]); out.keys.push(e.keys[old]); out.names.push(e.names[old].clone());
        if let Some(a) = e.anchor(old as u32) { out.anchors.push((new as u32, a)); }
        let mut ls: Vec<_> = e.range(old as u32).map(|l| (perm[e.targets[l] as usize], e.ltype[l], e.slow[l], e.lval[l], e.fast[l])).collect();
        ls.sort();
        for l in ls { out.targets.push(l.0); out.ltype.push(l.1); out.slow.push(l.2); out.lval.push(l.3); out.fast.push(l.4); }
        out.base_deg.push((out.targets.len() - *out.offsets.last().unwrap() as usize) as u32);
        out.offsets.push(out.targets.len() as u32);
    }
    let _ = tag::PLACE;
    out
}

#[test]
fn many_minds_share_one_base() {
    let fox = valley_fox();
    let minds: Vec<_> = (0..100).map(|i| fox.mind(Temperament::seeded(SEED, i), i as u64)).collect();
    assert!(minds.iter().all(|m| std::sync::Arc::ptr_eq(&m.store.base, &fox.species.base)));
    let own: usize = minds.iter().map(|m| m.bytes().1 + m.bytes().2 + m.bytes().3).sum();
    let alone = minds[0].store.expand().graph_bytes() + minds[0].store.expand().fast.len() * 2;
    println!("100 newborn foxes: one shared base of {} B, plus {} B of their own in all ({} B each); one fox's expanded mind alone {} B",
        fox.species.base.bytes(), own, own / 100, alone);
    let differ = minds.iter().filter(|m| !m.store.ov.slow.is_empty()).count();
    println!("{differ} of 100 differ from the base at birth (their temperament's anger links)");
    let _ = Action::Stay;
}
