//! Journeys: the tool-maker invents route and map tools from a traveller's own journeys and the
//! "how do I get there" questions it keeps thinking through. `cargo run --release --example tools_routes -- [seeds] [days]`
use fv_mind_lab::tools::routes::*;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let seeds: u64 = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(3);
    let days: u32 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(18);
    let show = a.get(3).is_some();
    for seed in 1..=seeds {
        let w = world(seed, &commuter_goals(seed));
        println!("\nseed {seed}: shortcut {:?} on day {SHORTCUT}, blocked {:?} on day {BLOCK}, dog at {} days {DOG}-{}", w.shortcut, w.blocked, w.blocked.1, DOG + 2);
        for (who, habit) in cast() {
            for v in [Variant::Think, Variant::Kit, Variant::Maker] {
                let r = travel(seed, who, habit, days, v, &w);
                let tot = |f: &dyn Fn(&Day) -> u64| r.days.iter().map(f).sum::<u64>();
                let steps = tot(&|d| d.steps as u64); let best = tot(&|d| d.best as u64);
                let per = |a: &[Day], f: &dyn Fn(&Day) -> u64| a.iter().map(f).sum::<u64>();
                let win = |from: u32, to: u32| { let s = &r.days[from as usize..(to as usize).min(r.days.len())]; format!("{}/{}", per(s, &|d| d.steps as u64), per(s, &|d| d.best as u64)) };
                println!("  {who} {:?} {:?}: journeys {}, steps/best {steps}/{best}, bumps {}, thought {} steps ({} link reads), tool {} steps ({} fuel), fell back {}, circles {}, shortcut used {}/{} | steps/best before shortcut {}, after {}, after block {}, dog days {}",
                    habit, v, tot(&|d| d.journeys as u64), tot(&|d| d.bumps as u64), tot(&|d| d.thought_steps as u64), tot(&|d| d.think_cost), tot(&|d| d.tool_steps as u64), tot(&|d| d.tool_fuel), tot(&|d| d.fell_back as u64), tot(&|d| d.circles as u64), tot(&|d| d.used_shortcut as u64), tot(&|d| d.could_shortcut as u64),
                    win(1, SHORTCUT), win(SHORTCUT, BLOCK), win(BLOCK, DOG), win(DOG, DOG + 3));
                for m in &r.made { println!("      night {}: made {} (acc {} on its history, {} B){}", m.0, m.1, m.2, m.3, m.4.as_ref().map_or(String::new(), |c| format!("; cheap version {c}"))); }
                if v == Variant::Maker { println!("      bounds: {:?}; belt {} B", r.bound, r.bytes); if show { for t in &r.final_tools { println!("{t}"); } } }
            }
        }
    }
}
