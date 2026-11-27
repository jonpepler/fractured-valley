//! Round 2, stage 3: the danger, teaching and hunt scenarios with every mind (people and animals)
//! thinking every N ticks (`people::sim::STUDY_EVERY`). One row per case and rate, for pairing.
//! `cargo run --release --example rate_scenarios -- <rates> <danger seeds> <teaching seeds> <hunt seeds> [tag]`
//! Writes `out/rate/{danger,teaching,hunt}<tag>.csv`. Cases run in parallel, one thread each;
//! every case is single-threaded and deterministic.
use fv_mind_lab::danger::runs::{encounter_with, Case as DCase, TEMPERS};
use fv_mind_lab::hunt::runs::{hunt, Case as HCase, Mode};
use fv_mind_lab::mind::{R_FIGHT, R_FLEE, R_HOLD};
use fv_mind_lab::people::runs::{s1, s4_chain};
use fv_mind_lab::people::sim::STUDY_EVERY;
use fv_mind_lab::people::world::SPRING;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

#[derive(Clone, Copy)]
enum Job { Danger(u32, u64, usize, bool, bool), Teach(u32, u64, u32), Hunt(u32, u64) }

fn main() {
    let a: Vec<String> = std::env::args().collect();
    // A rate N, or N with "g" (every tick while holding an intent: rows get every = 1000 + N).
    let rates: Vec<u32> = a.get(1).map(|x| x.split(',').map(|y| if let Some(n) = y.strip_suffix('g') { 1000 + n.parse::<u32>().unwrap() } else { y.parse().unwrap() }).collect()).unwrap_or(vec![1, 2, 4, 10, 20]);
    let ds: u64 = a.get(2).and_then(|x| x.parse().ok()).unwrap_or(50);
    let ts: u64 = a.get(3).and_then(|x| x.parse().ok()).unwrap_or(67);
    let hs: u64 = a.get(4).and_then(|x| x.parse().ok()).unwrap_or(200);
    let tag = a.get(5).cloned().unwrap_or_default();
    std::fs::create_dir_all("out/rate").unwrap();
    let f = |n: &str| Mutex::new(std::fs::File::create(format!("out/rate/{n}{tag}.csv")).unwrap());
    let (df, tf, hf) = (f("danger"), f("teaching"), f("hunt"));
    writeln!(df.lock().unwrap(), "every,seed,temper,allies,expected,error,arousal,stress,stress_peak,first,flee,fight,hold,all_responses,gored,struck,braced,boar_fled,down").unwrap();
    writeln!(tf.lock().unwrap(), "every,seed,set,told_spring,told_first_tick,untold_spring,told_sooner,cai_drank_spring,via_said,cai_err").unwrap();
    writeln!(hf.lock().unwrap(), "every,seed,planned_caught,planned_tick,solo_caught,solo_tick,planned_effort,solo_effort,proposed,joined,declined,breaches").unwrap();
    let mut jobs = vec![];
    for &e in &rates {
        for seed in 1..=ds { for t in 0..TEMPERS.len() { for allies in [false, true] { for expected in [false, true] { jobs.push(Job::Danger(e, seed, t, allies, expected)); } } } }
        for seed in 1..=ts { for set in 0..3 { jobs.push(Job::Teach(e, seed, set)); } }
        for seed in 1..=hs { jobs.push(Job::Hunt(e, seed)); }
    }
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map(|x| x.get()).unwrap_or(4);
    std::thread::scope(|sc| {
        for _ in 0..threads {
            sc.spawn(|| loop {
                let k = next.fetch_add(1, Ordering::Relaxed);
                let Some(&job) = jobs.get(k) else { break };
                match job {
                    Job::Danger(e, seed, t, allies, expected) => {
                        STUDY_EVERY.with(|c| c.set(e % 1000)); fv_mind_lab::people::sim::STUDY_ENGAGED.with(|c| c.set(e >= 1000));
                        let (o, _) = encounter_with(&DCase { seed, temper: TEMPERS[t].1, allies, expected }, 700, 2, false);
                        let all: u32 = o.counts.iter().sum();
                        writeln!(df.lock().unwrap(), "{e},{seed},{t},{},{},{},{},{},{},{},{},{},{},{all},{},{},{},{},{}", allies as u8, expected as u8, o.error, o.arousal, o.stress, o.stress_peak,
                            o.first.map(|r| r as i32).unwrap_or(-1), o.counts[R_FLEE as usize], o.counts[R_FIGHT as usize], o.counts[R_HOLD as usize], o.gored, o.struck, o.braced, o.boar_fled as u8, o.down_at.is_some() as u8).unwrap();
                    }
                    Job::Teach(e, seed, set) => {
                        STUDY_EVERY.with(|c| c.set(e % 1000)); fv_mind_lab::people::sim::STUDY_ENGAGED.with(|c| c.set(e >= 1000));
                        let (r, _) = s1(seed, set, true, 600);
                        let (u, _) = s1(seed, set, false, 600);
                        let (ch, _) = s4_chain(seed, set, true);
                        let sooner = r.first_drink.map(|d| d.0).unwrap_or(999) < u.first_drink.map(|d| d.0).unwrap_or(999);
                        writeln!(tf.lock().unwrap(), "{e},{seed},{set},{},{},{},{},{},{},{}", r.first_drink.is_some_and(|d| d.1 == SPRING) as u8, r.first_drink.map(|d| d.0 as i32).unwrap_or(301),
                            u.first_drink.is_some_and(|d| d.1 == SPRING) as u8, sooner as u8, ch.cai_drank_spring as u8, ch.via_said as u8, ch.cai_err).unwrap();
                    }
                    Job::Hunt(e, seed) => {
                        STUDY_EVERY.with(|c| c.set(e % 1000)); fv_mind_lab::people::sim::STUDY_ENGAGED.with(|c| c.set(e >= 1000));
                        let (p, _) = hunt(&HCase { seed, set: 1, mode: Mode::Planned });
                        let (s, _) = hunt(&HCase { seed, set: 1, mode: Mode::Solo });
                        writeln!(hf.lock().unwrap(), "{e},{seed},{},{},{},{},{},{},{},{},{},{}", p.caught.is_some() as u8, p.caught.map(|c| c.0 as i32).unwrap_or(-1), s.caught.is_some() as u8, s.caught.map(|c| c.0 as i32).unwrap_or(-1),
                            p.effort, s.effort, p.proposed.is_some() as u8, p.joined, p.declined, p.breaches.iter().sum::<u32>()).unwrap();
                    }
                }
            });
        }
    });
    eprintln!("done: {} cases", jobs.len());
}
