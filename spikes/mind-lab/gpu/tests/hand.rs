//! The GPU equals the CPU on hand-made and random graphs (incl. focus, courage, caps, budget).
use fv_mind_lab::graph::{lt, tag, Builder};
use fv_mind_lab::rng::hash;
use fv_mind_lab::store::{Base, Runtime};
use fv_mind_lab::think::{Activation, Form, Mods, Params};
use fv_mind_lab::think2;
use fv_mind_lab_gpu::{Gpu, Job};
use std::sync::Arc;

fn gpu() -> Option<Gpu> {
    match Gpu::new() { Ok(g) => { eprintln!("adapter: {} ({:?})", g.info.name, g.info.backend); Some(g) } Err(e) => { if std::env::var_os("FV_GPU_REQUIRED").is_some() { panic!("{e}") } eprintln!("skipped: {e}"); None } }
}

fn random(seed: u64, n: u32, deg: u32) -> Builder {
    let mut b = Builder::new(seed);
    for i in 0..n { let r = hash(seed, i as u64, 1, 0); b.node(&format!("n{i}"), (r % 2001) as i32 - 1000, ((r >> 12) % 256) as u8 & !tag::ANCHORED, None); }
    for i in 0..n { for j in 0..deg { let r = hash(seed, i as u64, j as u64, 2); b.link_v(i, (r % n as u64) as u32, ((r >> 20) % lt::COUNT as u64) as u8, ((r >> 32) % 1001) as i32, ((r >> 44) % 2001) as i32 - 1000); } }
    b
}

#[test]
fn gpu_matches_cpu_on_random_graphs_with_learning() {
    let Some(g) = gpu() else { return };
    let mut total = 0;
    for seed in 0..4u64 {
        let n = 400u32;
        let mut r = Runtime::new(Arc::new(Base::compile(random(seed, n, 10))));
        // Own nodes and links, fast weights, reweights: the patches.
        for i in 0..60u64 {
            let u = (hash(seed, i, 0, 5) % n as u64) as u32;
            let x = fv_mind_lab::store::Store::add_node(&mut r, "own", (hash(seed, i, 1, 5) % 2001) as i32 - 1000, tag::FOOD, None, seed);
            fv_mind_lab::store::Store::add_link(&mut r, u, x, lt::ASSOC, 600, 0);
            fv_mind_lab::store::Store::add_link(&mut r, x, (u + 3) % n, lt::KIND, 800, -200);
            let ls = fv_mind_lab::store::Store::links_vec(&r, u);
            fv_mind_lab::store::Store::set_fast(&mut r, ls[0].key, 150);
            if i % 3 == 0 { fv_mind_lab::store::Store::set_slow(&mut r, ls[ls.len() / 2].key, 20); }
        }
        let nn = r.len();
        let sb = r.base.sorted.get_or_init(|| think2::SortedBase::build(&r.base));
        let base = think2::pack_base(sb);
        let mind = { let v = r.sorted_view().unwrap(); think2::pack_mind(&r, &v) };
        let res = g.resident(&base, &mind);
        // A chain of thinks: each job's state is the CPU's state after the previous one.
        let mut st = Activation::new(nn);
        let mut jobs = vec![]; let mut want = vec![];
        for i in 0..300u64 {
            let q = |k| (hash(seed, i, k, 9) % 1001) as i32;
            let m = Mods { hunger: q(1), arousal: q(2), mood: q(3) * 2 - 1000, fatigue: q(4), stress: q(5), focus: if i % 2 == 0 { q(6) } else { 0 }, intent: (hash(seed, i, 7, 9) % nn as u64) as u32, hunger2: q(11) / 2, courage: if i % 3 == 0 { q(8) } else { 0 }, ally: (hash(seed, i, 10, 9) % nn as u64) as u32 };
            let p = Params { frontier: [48, 12, 64, 6][(i % 4) as usize], hops: 1 + (i % 5) as usize, active_cap: [256, 40, 8][(i % 3) as usize], ws_k: 8, budget: [6000, 1500, 60000][(i % 3) as usize], persist: (500 + (i % 7) * 70) as i32, ..Params::default() };
            let inj: Vec<(u32, i32)> = (0..(i % 6)).map(|k| ((hash(seed, i, k, 3) % nn as u64) as u32, (hash(seed, i, k, 4) % 3000) as i32 - 100 + if k == 0 && i % 11 == 0 { 30_000 } else { 0 })).collect();
            let mut s_in: Vec<(u32, i32)> = st.active.iter().map(|&x| (x, st.act[x as usize])).collect(); s_in.sort(); s_in.dedup();
            let o = { let v = r.sorted_view(); think2::think_in(&r, v.as_ref(), &mut st, &m, &inj, &p, None) };
            let s_out: Vec<(u32, i32)> = st.active.iter().map(|&x| (x, st.act[x as usize])).collect();
            jobs.push(Job { mind: 0, base: 0, n: nn as u32, mods: m, params: p, state: s_in, inject: inj });
            want.push((o, s_out));
            if i % 50 == 49 { st.damp(400); }
        }
        let (got, _) = g.think(&res, &jobs);
        for (k, (a, b)) in got.iter().zip(&want).enumerate() { assert_eq!(a, b, "seed {seed} think {k}"); }
        total += got.len();
        let flags = want.iter().fold(0, |f, x| f | x.0.flags);
        eprintln!("seed {seed}: {} thinks equal; flags seen {flags:b}", got.len());
    }
    assert!(total > 0);
}
