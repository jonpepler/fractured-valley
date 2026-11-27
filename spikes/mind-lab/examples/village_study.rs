//! Stage 5's study: many valleys (seeds), each lived for a month in full detail, with the cheap
//! tier for distant minds, and with place pruning. Writes `out/village/{runs,days,people}.csv`.
//! `cargo run --release --example village_study -- <seeds> <people> <days> [modes]`
//! (modes: a comma list of full, cheap, prune; default all three). Seeds run in parallel, one
//! thread each; every run is single-threaded and deterministic.
use fv_mind_lab::graph::lt;
use fv_mind_lab::village::runs::{feelings, run, stats, Setup, PLACE_CLASSES, SAY_KINDS};
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let seeds: usize = a.get(1).and_then(|x| x.parse().ok()).unwrap_or(100);
    let n: usize = a.get(2).and_then(|x| x.parse().ok()).unwrap_or(30);
    let days: u32 = a.get(3).and_then(|x| x.parse().ok()).unwrap_or(30);
    let modes: Vec<String> = a.get(4).map(|m| m.split(',').map(String::from).collect()).unwrap_or(vec!["full".into(), "cheap".into(), "prune".into()]);
    let tag = a.get(5).cloned().unwrap_or_default();
    std::fs::create_dir_all("out/village").unwrap();
    let f = |name: &str| Mutex::new(std::fs::File::create(format!("out/village/{name}{tag}.csv")).unwrap());
    let (runs, daysf, people) = (f("runs"), f("days"), f("people"));
    writeln!(runs.lock().unwrap(), "seed,mode,n,days,hash,brain_us_per_mind_tick,thinks,skipped,woken,meals,drinks,gossip,help,proposals,caught,gored,surprised,said,heard_per_tick,air_per_tick,trust0,trust1,warmth0,warmth1,warm_sat,cold_sat,total_b,own_b,places,pruned_places,pruned_links,danger_ticks,feast,danger_thought,surprised_per_1000_thinks,told,dup_ids,merged_told,small_trained,small_asked,small_acted,small_empty,small_unreachable,shadow_agree,shadow_disagree,shadow_none,small_fuel,carry_agree,carry_disagree").unwrap();
    writeln!(daysf.lock().unwrap(), "seed,mode,day,before_total,after_total,base_share,places_b,people_b,sources_b,plans_b,notes_b,other_own_b,fast_before,state,expect,layer,place_count,source_links,told,nodes,links,meals,drinks,gossip,help,proposals,caught,surprised").unwrap();
    writeln!(people.lock().unwrap(), "seed,mode,i,reactivity,aggression,honesty,helpful,places,waters,bushes,{},walked,{},danger,surprised,drinks,meals,best_friend,best_close,warmth_out,trust_out,warmth_in,gossip_said,gossip_heard,hunts_led,told,source_links",
        (0..PLACE_CLASSES).map(|k| format!("at{k}")).collect::<Vec<_>>().join(","), (0..SAY_KINDS).map(|k| format!("said{k}")).collect::<Vec<_>>().join(",")).unwrap();
    // `FV_SEED_FROM`: start at this seed (to continue a study in parts); default 1.
    let from: u64 = std::env::var("FV_SEED_FROM").ok().and_then(|x| x.parse().ok()).unwrap_or(1);
    let jobs: Vec<(u64, String)> = (from - 1..from - 1 + seeds as u64).flat_map(|s| modes.iter().map(move |m| (s + 1, m.clone()))).collect();
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map(|x| x.get()).unwrap_or(4);
    std::thread::scope(|sc| {
        for _ in 0..threads {
            sc.spawn(|| loop {
                let k = next.fetch_add(1, Ordering::Relaxed);
                let Some((seed, mode)) = jobs.get(k).cloned() else { break };
                let mut s = Setup::new(seed, n, days);
                s.apply_mode(&mode);
                let (rec, v) = run(&s, 1, None);
                let st = stats(&v);
                let (w0, t0) = feelings(seed, n);
                let mean0 = |m: &Vec<Vec<i32>>| m.iter().enumerate().flat_map(|(i, r)| r.iter().enumerate().filter(move |(j, _)| *j != i).map(|x| *x.1 as i64)).sum::<i64>() as f64 / (n * (n - 1)) as f64;
                let mut trust1 = 0i64; let mut warm1 = 0i64; let (mut hot, mut cold) = (0, 0);
                for p in &v.folk { for j in 0..n { if j == p.i { continue; } trust1 += p.trust(j) as i64; let w = p.warmth(j); warm1 += w as i64; if w >= 900 { hot += 1 } if w <= -900 { cold += 1 } } }
                let pairs = (n * (n - 1)) as f64;
                let last = rec.after.last().unwrap();
                let mean = |f: &dyn Fn(&fv_mind_lab::village::runs::Storage) -> usize| last.iter().map(f).sum::<usize>() as f64 / n as f64;
                let tot = |f: &dyn Fn(&fv_mind_lab::village::life::Stats) -> u32| st.iter().map(f).sum::<u32>();
                let sum_day = |f: &dyn Fn(&fv_mind_lab::village::runs::DayRec) -> u32| rec.day.iter().map(f).sum::<u32>();
                let pruned: (u32, u32) = v.folk.iter().fold((0, 0), |a, p| (a.0 + p.m.pruned.0, a.1 + p.m.pruned.1));
                let line = format!("{seed},{mode},{n},{days},{:016x},{:.2},{},{},{},{},{},{},{},{},{},{},{},{},{:.3},{:.3},{:.1},{:.1},{:.1},{:.1},{:.4},{:.4},{:.0},{:.0},{:.2},{},{},{},{},{},{:.2},{:.2},{},{},{}",
                    rec.hash, rec.brain_ns as f64 / 1e3 / (rec.ticks as f64 * n as f64), tot(&|s| s.thinks), tot(&|s| s.skipped), tot(&|s| s.woken),
                    sum_day(&|d| d.meals), sum_day(&|d| d.drinks), sum_day(&|d| d.gossip), sum_day(&|d| d.help), sum_day(&|d| d.proposals), sum_day(&|d| d.caught), sum_day(&|d| d.gored), sum_day(&|d| d.surprised), sum_day(&|d| d.said),
                    rec.heard as f64 / (rec.ticks as f64 * n as f64), rec.air as f64 / rec.ticks as f64,
                    mean0(&t0), trust1 as f64 / pairs, mean0(&w0), warm1 as f64 / pairs, hot as f64 / pairs, cold as f64 / pairs,
                    mean(&|s| s.total()), mean(&|s| s.own()), mean(&|s| s.place_count), pruned.0, pruned.1, rec.danger.iter().sum::<u32>(), sum_day(&|d| d.feast), rec.danger_thought.iter().sum::<u32>(), sum_day(&|d| d.surprised) as f64 * 1000.0 / tot(&|s| s.thinks).max(1) as f64,
                    v.folk.iter().map(|p| p.told.len()).sum::<usize>() as f64 / n as f64,
                    v.folk.iter().map(|p| { let mut ids: Vec<u32> = p.m.entity_nodes.iter().map(|x| x.0).collect(); ids.sort_unstable(); let k = ids.len(); ids.dedup(); k - ids.len() }).sum::<usize>(),
                    v.folk.iter().map(|p| p.merged_told).sum::<u32>(),
                    { let ss: Vec<fv_mind_lab::habit::SmallStats> = v.folk.iter().filter_map(|p| p.m.small.as_ref().map(|x| x.stats)).collect();
                      format!("{},{},{},{},{},{},{},{},{},{},{}", ss.iter().map(|x| x.trained).sum::<u32>(), ss.iter().map(|x| x.asked).sum::<u32>(), ss.iter().map(|x| x.acted).sum::<u32>(), ss.iter().map(|x| x.empty).sum::<u32>(), ss.iter().map(|x| x.unreachable).sum::<u32>(), ss.iter().map(|x| x.agree).sum::<u32>(), ss.iter().map(|x| x.disagree).sum::<u32>(), ss.iter().map(|x| x.no_answer).sum::<u32>(), ss.iter().map(|x| x.fuel).sum::<u64>(), ss.iter().map(|x| x.carry_agree).sum::<u32>(), ss.iter().map(|x| x.carry_disagree).sum::<u32>()) });
                writeln!(runs.lock().unwrap(), "{line}").unwrap();
                {
                    let mut df = daysf.lock().unwrap();
                    for d in 0..rec.before.len() {
                        let (b, af) = (&rec.before[d], &rec.after[d]);
                        let m = |v: &Vec<fv_mind_lab::village::runs::Storage>, f: &dyn Fn(&fv_mind_lab::village::runs::Storage) -> usize| v.iter().map(f).sum::<usize>() as f64 / n as f64;
                        let dr = rec.day.get(d).copied().unwrap_or_default();
                        writeln!(df, "{seed},{mode},{d},{:.0},{:.0},{:.0},{:.0},{:.0},{:.0},{:.0},{:.0},{:.0},{:.0},{:.0},{:.0},{:.0},{:.2},{:.2},{:.2},{:.1},{:.1},{},{},{},{},{},{},{}",
                            m(b, &|s| s.total()), m(af, &|s| s.total()), m(af, &|s| s.base_share), m(af, &|s| s.places), m(af, &|s| s.people), m(af, &|s| s.sources), m(af, &|s| s.plans), m(af, &|s| s.notes), m(af, &|s| s.other_own),
                            m(b, &|s| s.fast), m(af, &|s| s.state), m(af, &|s| s.expect), m(af, &|s| s.layer), m(af, &|s| s.place_count), m(af, &|s| s.source_links), m(af, &|s| s.told), m(af, &|s| s.nodes), m(af, &|s| s.links),
                            dr.meals, dr.drinks, dr.gossip, dr.help, dr.proposals, dr.caught, dr.surprised).unwrap();
                    }
                }
                if mode == "full" || mode == "cheap" || mode.starts_with('e') {
                    let mut pf = people.lock().unwrap();
                    let vs = v.w.village.as_ref().unwrap();
                    for (i, p) in v.folk.iter().enumerate() {
                        let (pl, wa, bu) = fv_mind_lab::village::life::knows(p);
                        let (bf, bc) = (0..n).filter(|&j| j != i).map(|j| (rec.close[i * n + j], j)).max().map(|x| (x.1, x.0)).unwrap_or((0, 0));
                        let wo = (0..n).filter(|&j| j != i).map(|j| p.warmth(j) as i64).sum::<i64>() / (n as i64 - 1);
                        let to = (0..n).filter(|&j| j != i).map(|j| p.trust(j) as i64).sum::<i64>() / (n as i64 - 1);
                        let wi = v.folk.iter().filter(|q| q.i != i).map(|q| q.warmth(i) as i64).sum::<i64>() / (n as i64 - 1);
                        let sl = p.m.store.ov.added.iter().filter(|l| l.ty == lt::SOURCE).count();
                        writeln!(pf, "{seed},{mode},{i},{},{},{},{},{pl},{wa},{bu},{},{},{},{},{},{},{},{bf},{bc},{wo},{to},{wi},{},{},{},{},{sl}",
                            p.m.temperament.reactivity, p.m.temperament.aggression, p.traits.honesty, p.traits.helpful,
                            rec.at[i].iter().map(|x| x.to_string()).collect::<Vec<_>>().join(","), rec.walked[i], rec.said[i].iter().map(|x| x.to_string()).collect::<Vec<_>>().join(","),
                            rec.danger[i], rec.surprised[i], v.w.people[i].drinks, vs.meals[i], st[i].gossip_said, st[i].gossip_heard, st[i].hunts_led, p.told.len()).unwrap();
                    }
                }
                eprintln!("seed {seed} {mode} done");
            });
        }
    });
}
