//! Bryn finds Ash: thinking it through, wandering, or asking a tool it built in its sleep.
//! `cargo run --release --example tools_ash -- [seeds] [days] [people]`
use fv_mind_lab::tools::ash::*;
use fv_mind_lab::tools::kit;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let seeds: u64 = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(4);
    let days: u32 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(16);
    let people: usize = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(12);
    let (mut all_w, mut all_t, mut all_k, mut n_all, mut wins, mut losses) = (vec![], vec![], vec![], 0, 0, 0);
    for seed in 1..=seeds {
        let d = run(seed, people, days, kit::MAKER);
        let after: Vec<&Question> = d.qs.iter().filter(|q| q.tool.is_some()).collect();
        let f = |q: &Question, o: &[usize]| time_to_find(&d, q, o).unwrap_or(GIVE_UP);
        let (mut w, mut t, mut k) = (vec![], vec![], vec![]);
        for q in &after { w.push(f(q, &[])); t.push(f(q, &q.think)); let kk = f(q, q.tool.as_ref().unwrap()); k.push(kk); if kk < *t.last().unwrap() { wins += 1; } else if kk > *t.last().unwrap() { losses += 1; } }
        let mean = |v: &[u32]| if v.is_empty() { 0 } else { v.iter().sum::<u32>() / v.len() as u32 };
        let found = |v: &[u32]| v.iter().filter(|&&x| x < GIVE_UP).count();
        let tc: u64 = d.qs.iter().map(|q| q.think_cost as u64).sum::<u64>() / d.qs.len().max(1) as u64;
        let tf: i64 = after.iter().map(|q| q.tool_fuel).sum::<i64>() / after.len().max(1) as i64;
        let tns: u64 = d.qs.iter().map(|q| q.think_ns).sum::<u64>() / d.qs.len().max(1) as u64;
        let kns: u64 = after.iter().map(|q| q.tool_ns).sum::<u64>() / after.len().max(1) as u64;
        let sv: u64 = after.iter().map(|q| q.step_visits as u64).sum::<u64>() / after.len().max(1) as u64;
        let fired = after.iter().filter(|q| q.fired).count();
        let cm = after.iter().filter(|q| q.came_to_mind == Some(true)).count();
        println!("seed {seed}: {} sightings of Ash, {} questions, tool from night {:?}; on the {} questions after: mean ticks to find (found of {}): wander {} ({}), think {} ({}), tool {} ({}); tool's binding fired {fired}, answer came to mind {cm}",
            d.sightings, d.qs.len(), d.made.first().map(|m| m.0), after.len(), after.len(), mean(&w), found(&w), mean(&t), found(&t), mean(&k), found(&k));
        println!("    cost a question: thinking {tc} link reads ({tns} ns); tool {tf} fuel ({kns} ns, the whole brain-tick call); the clone's two steps {sv} link reads; a link read {:.1} ns; belt {} B", d.link_ns, d.belt_bytes);
        for m in &d.made { println!("    night {}: {}", m.0, m.1); }
        if seed == 1 { if let Some(m) = d.made.last() { println!("{}", m.2); } }
        all_w.extend(w); all_t.extend(t); all_k.extend(k); n_all += after.len();
    }
    let mean = |v: &[u32]| if v.is_empty() { 0 } else { v.iter().sum::<u32>() / v.len() as u32 };
    println!("\nall seeds, {n_all} questions after a tool: mean ticks to find: wander {}, think {}, tool {}; tool faster than thinking on {wins}, slower on {losses}", mean(&all_w), mean(&all_t), mean(&all_k));
}
