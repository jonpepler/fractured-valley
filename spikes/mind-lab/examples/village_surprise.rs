//! What surprises villagers: percepts with a prediction error at or above the bar, by kind, per day.
use fv_mind_lab::village::runs::{build, set_tiers, Setup};
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let days: u32 = args.get(1).and_then(|x| x.parse().ok()).unwrap_or(30);
    let seed: u64 = args.get(2).and_then(|x| x.parse().ok()).unwrap_or(1);
    let s = Setup::new(seed, 30, days);
    let mut v = build(&s);
    let ord: Vec<usize> = (0..30).collect();
    let mut by = std::collections::BTreeMap::<String, u32>::new();
    let mut danger = 0;
    for t in 0..days * 301 {
        set_tiers(&mut v, &s);
        v.step(4, &ord);
        for p in &v.folk {
            if p.m.held.response.is_some() { danger += 1; }
            for x in p.m.last.percepts.iter().filter(|x| x.error >= 400) {
                let k = if x.kind < 4 || x.kind >= 16 { "a person".to_string() } else { p.m.kinds[x.kind as usize].name.to_string() };
                let tracked = if p.pnode.contains(&x.node) { " (known)" } else { "" };
                *by.entry(format!("{k}{tracked} plan:{}", p.m.plan_expect(v.w.tick - 1, x.node).is_some())).or_default() += 1;
            }
            for a in &p.m.last.absent { *by.entry(format!("absent {}", p.m.kinds[a.1 as usize].name)).or_default() += 1; }
        }
        if (t + 1) % 301 == 0 && (t + 1) / 301 % 5 == 0 { println!("day {}: {:?}; danger ticks {danger}; boar at {:?} meals {:?}", (t + 1) / 301, by, (v.w.boars[0].x, v.w.boars[0].y), v.w.village.as_ref().unwrap().boar_meals); by.clear(); }
    }
}
