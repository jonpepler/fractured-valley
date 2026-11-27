use fv_thought_graph::scenarios::{dog_charges, CHARGE_AT, SEED};
use fv_thought_graph::world::Action;
fn main() {
    for id in 0..16u32 {
        let mut s = dog_charges(SEED, id);
        let t = s.m.temperament;
        let mut line = format!("id{id:2} react {:4} aggr {:4} |", t.reactivity, t.aggression);
        for _ in 0..14 {
            let a = s.step();
            let r = &s.m.last;
            if s.w.tick - 1 >= CHARGE_AT {
                let c = match a { Action::Flee{speed,..} => format!("F{speed}"), Action::Snap{..} => "S".into(), Action::Stay => ".".into(), Action::WalkTo{..} => "w".into(), _ => "?".into() };
                line += &format!(" {c}/h{}v{}s{}", r.hops_run, r.visits, s.m.mods.stress);
            }
        }
        let bites = s.w.log.iter().filter(|l| l.1.contains("bites")).count();
        println!("{line} bites {bites}");
    }
}
