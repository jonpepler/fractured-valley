//! cargo run --release --example home [variant] — a tick log of the home experiment.
use fv_thought_graph::scenarios::{self, HomeMove, SEED, DAY};
use fv_thought_graph::world::{Action, Kind};
fn main() {
    let v = match std::env::args().nth(1).as_deref() { Some("far") => HomeMove::Far, Some("mid") => HomeMove::Mid, Some("removed") => HomeMove::Removed, Some("second") => HomeMove::Second, Some("daily") => HomeMove::Daily, Some("none") => HomeMove::None, _ => HomeMove::Near };
    let days = 4;
    let mut s = scenarios::home_moved(SEED, v, days);
    let every: u32 = std::env::args().nth(2).and_then(|x| x.parse().ok()).unwrap_or(10);
    for _ in 0..days * DAY {
        let t = s.w.tick;
        let a = s.step();
        let m = &s.m; let r = &m.last;
        let interesting = !r.absent.is_empty() || !r.learnt.is_empty() || r.percepts.iter().any(|p| p.kind == Kind::Den && p.error >= 400) || t % every == 0 || matches!(a, Action::Flee{..});
        if interesting {
            let ws: Vec<&str> = r.ws.iter().take(6).map(|&n| m.g.names[n as usize].as_str()).collect();
            let home = m.g.names[m.home() as usize].clone(); let live = fv_thought_graph::spread::Live { g: &m.g, fast: &m.fast }; let tired = fv_thought_graph::spread::Mods { fatigue: 1000, ..Default::default() }; let fa = m.felt_with(&live, m.node("den"), &tired); let fb = m.felt_with(&live, m.node("new place 1"), &tired);
            println!("t{t:4} ({:2},{:2}) ar{:4} st{:4} mood{:5} fat{:4} loss{:4} pz{:?} absent{:?} learnt{:?} home={home} A{fa} B{fb} | {} | {}",
                s.w.fox.x, s.w.fox.y, m.mods.arousal, m.mods.stress, m.mods.mood, m.mods.fatigue, m.act(m.ids.loss), m.puzzle.map(|p| p.strength), r.absent.iter().map(|x| x.2).collect::<Vec<_>>(), r.learnt, r.why, ws.join(","));
        }
    }
}
