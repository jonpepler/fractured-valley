use fv_thought_graph::sim::Sim;
use fv_thought_graph::world::{Action, Event};
fn first_goal(h: i32, a: i32, mood: i32, nodog_day: bool) -> String {
    let mut s = Sim::new(7);
    if nodog_day { s.at(0, Event::NoDog); s.at(300, Event::Night); s.at(301, Event::SetHunger(h)); s.w.fox.hunger = 700; s.run(301); }
    else { s.w.fox.hunger = h; }
    s.m.pin_arousal = Some(a); if mood != 0 { s.m.pin_mood = Some(mood); }
    let mut last = String::from("stay");
    for _ in 0..12 { let act = s.step(); if let Action::WalkTo{..} = act { last = s.m.g.names[s.m.goal.unwrap() as usize].clone(); } else if let Action::Stay = act { last = "stay".into(); } }
    last
}
fn main() {
    print!("A\\H  ");
    for h in (300..=1000).step_by(100) { print!("{h:>12}"); } println!();
    for a in [0, 100, 200, 300, 400, 500, 600, 800] {
        print!("{a:>4} "); for h in (300..=1000).step_by(100) { print!("{:>12}", first_goal(h, a, 0, false)); } println!();
    }
    println!("mood -600:"); print!("   0 "); for h in (300..=1000).step_by(100) { print!("{:>12}", first_goal(h, 0, -600, false)); } println!();
    println!("day 2 after a day without the dog:"); print!("   0 "); for h in (300..=1000).step_by(100) { print!("{:>12}", first_goal(h, 0, 0, true)); } println!();
}
