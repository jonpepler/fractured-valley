//! One-command benchmark of think v2 on real minds, CPU against GPU:
//! `cargo run --release --bin gpu-bench -- [copies 300,3000] [ticks 20] [people 30] [days 2]`
//!
//! A village (people, boars, deer, foxes: four species bases) lives `days` days on the CPU, then
//! `ticks` more ticks with every think recorded. Each mind is copied `copies / minds` times (each
//! copy its own memory, as separate minds would be), the graphs frozen as they ended. One "step" is
//! one think of one mind, replaying a recorded think's inputs. The CPU runs every step with think
//! v1 and v2 on one thread and v2 on all threads; the GPU runs each tick's thinks (one per copy)
//! in one dispatch, graphs resident, inputs uploaded and outputs read back every tick. Every GPU
//! output is compared with the CPU's v2 output.
use fv_mind_lab::people::sim::Valley;
use fv_mind_lab::store::Runtime;
use fv_mind_lab::think::{self, Activation, Form};
use fv_mind_lab::think2::{self, Out, Rec};
use fv_mind_lab::village::runs::{run, Setup};
use fv_mind_lab_gpu::{Cost, Gpu, Job};
use std::cell::RefCell;
use std::rc::Rc;

fn minds(v: &Valley) -> Vec<&Runtime> {
    v.folk.iter().map(|p| &p.m.store).chain(v.beasts.iter().chain(&v.deer_minds).chain(&v.fox_minds).map(|m| &m.store)).collect()
}
fn uid(r: &Runtime) -> u64 { r.cache.0.lock().unwrap().uid }

/// One think on the CPU from a record's inputs: load the state, think, read the state back.
fn cpu_think(f: &Runtime, st: &mut Activation, r: &Rec, v2: bool) -> (Out, Vec<(u32, i32)>) {
    st.ensure(f.len());
    for &(n, a) in &r.state_in { st.act[n as usize] = a; st.active.push(n); }
    let out = if v2 { let v = f.sorted_view(); think2::think_in(f, v.as_ref(), st, &r.mods, &r.inject, &r.params, None) }
        else { let ws = think::think(f, st, &r.mods, &r.inject, &r.params, None); Out { ws, visits: st.visits, hops_run: st.hops_run, flags: 0 } };
    let s: Vec<(u32, i32)> = st.active.iter().map(|&n| (n, st.act[n as usize])).collect();
    st.clear();
    (out, s)
}

fn main() {
    std::env::set_var("FV_THINK", "v2");
    let a: Vec<String> = std::env::args().collect();
    let copies: Vec<usize> = a.get(1).map(|x| x.split(',').map(|y| y.parse().unwrap()).collect()).unwrap_or(vec![300, 3000]);
    let ticks: usize = a.get(2).and_then(|x| x.parse().ok()).unwrap_or(20);
    let people: usize = a.get(3).and_then(|x| x.parse().ok()).unwrap_or(30);
    let days: u32 = a.get(4).and_then(|x| x.parse().ok()).unwrap_or(2);
    let threads = std::thread::available_parallelism().map(|x| x.get()).unwrap_or(4);
    let g = match Gpu::new() { Ok(g) => g, Err(e) => { println!("{e}"); std::process::exit(2) } };
    println!("adapter: {} ({:?}, {:?}); CPU threads: {threads}", g.info.name, g.info.device_type, g.info.backend);

    // The village, then `ticks` recorded ticks.
    let (_, mut v) = run(&Setup::new(1, people, days), 1, None);
    let recs: Rc<RefCell<Vec<Rec>>> = Rc::new(RefCell::new(vec![]));
    let r2 = recs.clone();
    think2::record_start(1 << 30, Box::new(move |b| r2.borrow_mut().extend(b.recs)));
    let order: Vec<usize> = (0..v.folk.len()).collect();
    for _ in 0..ticks { v.step(1, &order); }
    think2::record_stop();
    let all = minds(&v);
    let uids: Vec<u64> = all.iter().map(|m| uid(m)).collect();
    let mut per: Vec<Vec<Rec>> = vec![vec![]; all.len()];
    for r in recs.borrow().iter() { if let Some(i) = uids.iter().position(|&u| u == r.uid) { per[i].push(r.clone()); } }
    let live: Vec<usize> = (0..all.len()).filter(|&i| !per[i].is_empty()).collect();
    let nrec: usize = per.iter().map(|x| x.len()).sum();
    let bases: Vec<u64> = { let mut b: Vec<u64> = live.iter().map(|&i| all[i].base.sorted.get().unwrap().uid).collect(); b.sort(); b.dedup(); b };
    println!("village: {people} people, {days} days, then {ticks} ticks recorded: {} minds that thought, {} species bases, {nrec} thinks", live.len(), bases.len());

    for &m in &copies {
        // Copies: copy c is mind live[c % live.len()], its own clone; its think at tick t is that
        // mind's recorded think (c / live.len() + t) mod its count.
        let src: Vec<usize> = (0..m).map(|c| live[c % live.len()]).collect();
        let stores: Vec<Runtime> = src.iter().map(|&i| all[i].clone()).collect();
        for s in &stores { drop(s.sorted_view()); }
        let pick = |c: usize, t: usize| -> &Rec { let l = &per[src[c]]; &l[(c / live.len() + t) % l.len()] };
        let steps = m * ticks;
        // CPU, one thread: v1, then v2 (whose outputs are kept for the check).
        let mut st = Activation::default();
        let t0 = std::time::Instant::now();
        for t in 0..ticks { for c in 0..m { std::hint::black_box(cpu_think(&stores[c], &mut st, pick(c, t), false)); } }
        let v1_1 = steps as f64 / t0.elapsed().as_secs_f64();
        think2::take_stats();
        let t0 = std::time::Instant::now();
        let mut want = Vec::with_capacity(steps);
        for t in 0..ticks { for c in 0..m { want.push(cpu_think(&stores[c], &mut st, pick(c, t), true)); } }
        let v2_1 = steps as f64 / t0.elapsed().as_secs_f64();
        let s = think2::take_stats();
        // CPU, all threads: copies split between threads that live for the whole run; a barrier
        // after each tick, as a sim would have.
        let t0 = std::time::Instant::now();
        let bar = std::sync::Barrier::new(threads);
        std::thread::scope(|sc| {
            let chunk = m.div_ceil(threads);
            for k in 0..threads {
                let (stores, pick, bar) = (&stores, &pick, &bar);
                sc.spawn(move || { let mut st = Activation::default(); for t in 0..ticks { for c in k * chunk..((k + 1) * chunk).min(m) { std::hint::black_box(cpu_think(&stores[c], &mut st, pick(c, t), true)); } bar.wait(); } });
            }
        });
        let v2_n = steps as f64 / t0.elapsed().as_secs_f64();
        // GPU: bases and every copy's mind resident; one dispatch per tick.
        let mut bw = vec![]; let mut base_at = std::collections::HashMap::new();
        let mut mw = vec![]; let mut mind_at = vec![];
        for s in &stores {
            let sb = s.base.sorted.get().unwrap();
            base_at.entry(sb.uid).or_insert_with(|| { let o = bw.len() as u32; bw.extend(think2::pack_base(sb)); o });
            mind_at.push(mw.len() as u32);
            let v = s.sorted_view().unwrap();
            mw.extend(think2::pack_mind(s, &v));
        }
        let res = g.resident(&bw, &mw);
        let jobs_at = |t: usize| -> Vec<Job> { (0..m).map(|c| { let r = pick(c, t); let s = &stores[c]; Job { mind: mind_at[c], base: base_at[&s.base.sorted.get().unwrap().uid], n: s.len() as u32, mods: r.mods, params: r.params, state: r.state_in.clone(), inject: r.inject.clone() } }).collect() };
        let jobs: Vec<Vec<Job>> = (0..ticks).map(jobs_at).collect();
        let mut same = true; let mut bad = 0;
        let _ = g.think(&res, &jobs[0]); // warm-up (pipeline, buffers)
        let mut cost = Cost::default();
        let mut best = f64::MAX;
        for pass in 0..3 {
            let t0 = std::time::Instant::now();
            let mut c = Cost::default();
            for (t, j) in jobs.iter().enumerate() {
                let (out, k) = g.think(&res, j);
                c += k;
                if pass == 0 { for (i, o) in out.iter().enumerate() { if *o != want[t * m + i] { same = false; bad += 1; } } }
            }
            let el = t0.elapsed().as_secs_f64();
            if el < best { best = el; cost = c; }
        }
        let gpu = steps as f64 / best;
        let per_think = |x: u64| x as f64 / steps as f64;
        println!("\n{m} minds × {ticks} ticks = {steps} steps (graphs resident: bases {:.1} MB, minds {:.1} MB, {:.1} KB a mind)", bw.len() as f64 * 4e-6, mw.len() as f64 * 4e-6, mw.len() as f64 * 4e-3 / m as f64);
        println!("GPU matches CPU: {same}{}", if same { String::new() } else { format!(" ({bad} of {steps} differ)") });
        println!("steps per second: CPU v1 1 thread {v1_1:.0}; CPU v2 1 thread {v2_1:.0}; CPU v2 {threads} threads {v2_n:.0}; GPU {gpu:.0} (in the dispatch alone {:.0})", steps as f64 / cost.run_s);
        println!("GPU cost structure: {} dispatches ({} a tick, all hops inside), {} workgroups of 64; up {:.0} B and down {:.0} B a step; time up {:.0}% / dispatch and wait {:.0}% / read back {:.0}%",
            cost.dispatches, cost.dispatches / ticks as u64, (m as u32).min(g.workgroups), per_think(cost.bytes_up), per_think(cost.bytes_down), cost.up_s / best * 100.0, cost.run_s / best * 100.0, cost.down_s / best * 100.0);
        println!("per step (from the CPU's count): {:.1} senders, {:.1} links read, {:.1} skipped by the early stop, {:.1} flows (an atomic add each, plus {:.1} first touches), {:.1} active before the cap",
            per_think(s.senders), per_think(s.evaluated), per_think(s.stopped), per_think(s.flows), per_think(s.touched), per_think(s.active_sum));
        if !same { std::process::exit(1); }
    }
}
