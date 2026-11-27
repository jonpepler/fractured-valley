//! Prints the backlog, fatigue and consolidation over the dog day, a quiet day and sleepless days.
//! cargo run --release --example pressure
use fv_thought_graph::scenarios::{self, NIGHT, SEED};
use fv_thought_graph::sim::Sim;
use fv_thought_graph::world::Action;

fn run(label: &str, mut s: Sim, ticks: u32) {
    println!("== {label}");
    let (mut visits, mut cons, mut n, mut idle) = (0i64, 0i64, 0i64, 0);
    for t in 0..ticks {
        let a = s.step();
        let r = &s.m.last;
        visits += r.visits as i64; cons += r.consolidated; n += 1; if r.consolidated > 0 { idle += 1; }
        if t % 50 == 0 || (14..40).contains(&t) && t % 4 == 0 || t == NIGHT || t == NIGHT + 1 {
            println!("t{t:4} fox ({:2},{:2}) backlog {:6} fatigue {:4} eps {:2} fast {:6} visits {:3} cons {:4} stress {:3} mood {:4} {:?} {}",
                s.w.fox.x, s.w.fox.y, s.m.backlog(), s.m.mods.fatigue, s.m.episodes.len(), s.m.fast_total(), r.visits, r.consolidated, s.m.mods.stress, s.m.mods.mood,
                match a { Action::WalkTo{..} => "walk", Action::Stay => "stay", Action::Flee{..} => "flee", Action::Eat{..} => "eat", Action::Sleep => "sleep", Action::Snap{..} => "snap" }, r.why);
        }
    }
    let hh = s.m.node("henhouse");
    let dl = s.m.link_of(hh, s.m.ids.danger, fv_thought_graph::graph::lt::FEELS).unwrap();
    println!("   mean visits {} mean consolidation {} idle ticks {idle}/{n}; henhouse→danger slow {} fast {}; consol {:?}", visits / n, cons / n, s.m.g.slow[dl], s.m.fast[dl], s.m.consol);
}

fn main() {
    run("dog day, sleeps", scenarios::dog_day(SEED), NIGHT + 150);
    run("dog day, no sleep", scenarios::dog_day_no_sleep(SEED), NIGHT + 150);
    run("quiet day (no dog), sleeps", scenarios::no_dog_day(SEED), NIGHT + 150);
    let mut s = scenarios::no_dog_day(SEED); s.m.skip_sleep = true;
    run("quiet, no sleep", s, NIGHT + 150);
}
