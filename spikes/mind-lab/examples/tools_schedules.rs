//! The schedule alternatives compared, and the tool-maker on the same streams.
//! `cargo run --release --example tools_schedules -- [routine|village] [seeds] [days]`
use fv_mind_lab::tools::{data, kit, study};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let which = a.get(1).map(String::as_str).unwrap_or("routine");
    let seeds: u64 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(4);
    let days: u32 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(30);
    let descs: Vec<_> = kit::SCHEDULES.iter().map(|s| kit::load(s)).collect();
    let mut tot: Vec<study::Score> = vec![];
    let mut makers: Vec<(u64, Vec<study::MakerRun>)> = vec![];
    for seed in 1..=seeds {
        let st = match which { "village" => data::village(seed, 20, days, &[1, 2, 3, 4, 5], 6), _ => data::routine(seed, 8, days, &[1, 2, 3], 100) };
        eprintln!("{}: {} events", st.name, st.evs.len());
        let sc = study::schedules(&st, &descs, None, None, true);
        if tot.is_empty() { tot = sc.clone(); for t in tot.iter_mut() { t.per_day.clear(); } }
        for (t, s) in tot.iter_mut().zip(&sc) {
            t.n += s.n; t.hit1 += s.hit1; t.hit3 += s.hit3; t.bytes += s.bytes; t.asks += s.asks; t.ask_fuel += s.ask_fuel; t.ask_ns += s.ask_ns; t.observe_fuel += s.observe_fuel; t.observes += s.observes;
            if t.per_day.len() < s.per_day.len() { t.per_day.resize(s.per_day.len(), (0, 0)); }
            for (d, p) in s.per_day.iter().enumerate() { t.per_day[d].0 += p.0; t.per_day[d].1 += p.1; }
        }
        let runs = study::with_maker(&st, kit::MAKER, 0, 400);
        let from = days as usize * 2 / 3;
        for r in &runs {
            let hw = study::schedules(&st, &descs, Some(0), Some(r.observer), false);
            let (n, h) = r.score.tail(from);
            let others: Vec<String> = hw.iter().map(|s| { let (n, h) = s.tail(from); format!("{} {}", s.name, if n == 0 { 0 } else { h * 1000 / n }) }).collect();
            println!("seed {seed} observer {}: last third, made tool {} ({} questions) | {}", r.observer, if n == 0 { 0 } else { h * 1000 / n }, n, others.join(", "));
        }
        makers.push((seed, runs));
    }
    println!("\n{which}: {seeds} seeds × {days} days; questions every {} ticks about everyone", data::ASK_EVERY);
    println!("{:>18} {:>6} {:>6} {:>8} {:>8} {:>8} {:>7} {:>7} {:>6}  day 0,1,2,4,8,16,last", "tool", "top1", "top3", "state B", "desc B", "fuel/ask", "ns/ask", "bound", "learnt");
    for t in &tot {
        let curve: Vec<String> = [0usize, 1, 2, 4, 8, 16, t.per_day.len() - 1].iter().map(|&d| format!("{}", t.day_acc(d))).collect();
        println!("{:>18} {:>6} {:>6} {:>8} {:>8} {:>8} {:>7} {:>7} {:>6}  {}", t.name, t.acc1(), t.acc3(), t.bytes / seeds as usize, t.desc_bytes, t.ask_fuel / t.asks.max(1), t.ask_ns / t.asks.max(1), t.bound, t.learnt_by(900), curve.join(","));
    }
    println!("\ntool-maker, asking where person 0 is (thinking cost 400 link reads a question):");
    for (seed, runs) in &makers {
        for r in runs {
            let first = r.made.first();
            println!("  seed {seed} observer {}: thought {} questions; made {} tools; top1 {} when answered; {}", r.observer, r.thought, r.made.len(), r.score.acc1(),
                first.map_or("nothing".to_string(), |m| format!("night {} → {} (acc {} on its history, {} B, {} fuel/ask, tried {}/{})", m.0 / data::DAY, m.2, m.1.acc, m.1.bytes, m.1.fuel_per_ask, m.1.tried, m.1.of)));
            if let Some(m) = first { if !m.3.is_empty() { println!("      cheap version for the fast brain: {}", m.3); } }
            for m in r.made.iter().skip(1) { println!("      later, night {} → {} (acc {})", m.0 / data::DAY, m.2, m.1.acc); }
        }
    }
}
