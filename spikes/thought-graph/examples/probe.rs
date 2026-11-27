use fv_thought_graph::sim::Sim;
use fv_thought_graph::world::Event;
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let which = args.get(1).map(|s| s.as_str()).unwrap_or("hungry");
    let hunger: i32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(700);
    let arousal: i32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
    let mood: i32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
    let ticks: u32 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(40);
    let mut s = Sim::new(7);
    s.w.fox.hunger = hunger; s.m.mods.arousal = arousal; s.m.mods.mood = mood;
    if which == "fire" { s.at(22, Event::FireAt { x: 34, y: 17 }); }
    if which == "bitefire" { s.at(300, Event::Night); s.at(301, Event::SetHunger(hunger)); s.at(326, Event::FireAt { x: 35, y: 16 }); }
    if which == "bite" { s.at(300, Event::Night); s.at(301, Event::SetHunger(hunger)); }
    let skip: u32 = args.get(6).and_then(|s| s.parse().ok()).unwrap_or(0);
    for _ in 0..ticks {
        let a = s.step();
        let m = &s.m;
        let ws: Vec<String> = m.last.ws.iter().map(|&n| format!("{}:{}", m.g.names[n as usize], m.act(n))).collect();
        let sc: Vec<String> = m.last.scores.iter().map(|&(p, a, f)| format!("{}={}+{}", m.g.names[p as usize], a, f)).collect();
        let ev: Vec<String> = s.w.log.iter().filter(|l| l.0 + 1 == s.w.tick).map(|l| l.1.clone()).collect();
        if s.w.tick - 1 < skip { continue; }
        println!("t{:3} fox({:2},{:2}) h{} A{} M{} | {:?} | {} | {} | {}", s.w.tick - 1, s.w.fox.x, s.w.fox.y, s.w.fox.hunger, m.mods.arousal, m.mods.mood, a, ws.join(" "), sc.join(" "), ev.join("; "));
    }
}
