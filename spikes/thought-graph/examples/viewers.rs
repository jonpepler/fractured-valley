//! Writes one self-contained viewer page per scenario into `viewer/`, and `viewer/frames.json`,
//! the key moments `tools/frames.js` turns into pictures.
//!
//! cargo run --release --example viewers && node tools/frames.cjs
use fv_thought_graph::hd::Hd;
use fv_thought_graph::scenarios::{self, HomeMove, DAY, DAY2, FIRE_AT, NIGHT, SEED};
use fv_thought_graph::sim::Sim;
use fv_thought_graph::spread::Live;
use fv_thought_graph::viewer::{page, Key};
use fv_thought_graph::world::Action;
use std::fmt::Write;

fn first(s: &Sim, from: u32, f: impl Fn(&fv_thought_graph::sim::Frame) -> bool) -> u32 {
    s.frames.iter().find(|x| x.tick >= from && f(x)).map(|x| x.tick).unwrap_or(from)
}

fn main() {
    std::fs::create_dir_all("viewer").unwrap();
    let mut frames = String::from("[\n");
    let out = |file: &str, html: String, keys: &[Key], frames: &mut String| {
        std::fs::write(format!("viewer/{file}.html"), html).unwrap();
        for (i, (t, h, label)) in keys.iter().enumerate() {
            writeln!(frames, "  {{\"file\": \"{file}.html\", \"t\": {t}, \"h\": {}, \"name\": \"{file}-{}\", \"label\": \"{label}\"}},", h.map(|h| h as i64).unwrap_or(-1), i + 1).unwrap();
        }
        println!("viewer/{file}.html");
    };

    // The dog, a night's sleep, and the next morning.
    let mut s = scenarios::dog_day(SEED).recording();
    s.run(DAY2 + 40);
    let approach = first(&s, 0, |f| f.percepts.iter().any(|p| p.1 >= 400));
    let bite = first(&s, 0, |f| f.events.iter().any(|e| e.contains("bites")));
    let berries = first(&s, bite, |f| f.why.contains("berry bush"));
    let eats = first(&s, bite, |f| matches!(f.action, Action::Eat { .. }));
    let nap = first(&s, bite, |f| f.why.starts_with("tired"));
    let keys: Vec<Key> = vec![
        (2, Some(1), "Hunger spreads: hunger → food → the hens and the berries".into()),
        (6, None, "Hungry and calm: it heads for the henhouse, the nearer and richer food".into()),
        (approach, None, "The dog charges: a prediction error interrupts the thought".into()),
        (bite, None, "Bitten: pain and danger fill the workspace; it flees".into()),
        (berries, None, "Fast weights make the henhouse feel bad; it turns to the berry bush".into()),
        (nap, None, "The bite filled its backlog: tired, it naps at the den and consolidates".into()),
        (eats, None, "It eats at the berry bush".into()),
        (DAY2 + 4, None, "Next morning, after sleep: fast weights are empty, but the henhouse still feels bad".into()),
    ];
    let html = page(&s, "A bad day at the henhouse, and a night's sleep",
        "Day 1: a hungry fox goes for the hens, is bitten by the dog, and turns to the berries. Night: episodes are replayed and fast weights consolidated. Day 2: the henhouse carries the fear.",
        "graph", None, &keys);
    out("dog-and-sleep", html, &keys, &mut frames);

    // The meadow fire (day 2 of the same story), in the graph form and the compiled form.
    for compiled in [false, true] {
        let mut s = scenarios::meadow_fire(SEED).recording();
        s.record_from = NIGHT + 1;
        s.run(NIGHT + 1);
        if compiled { s.m.use_compiled(2048, SEED); }
        s.run(80);
        let camp = first(&s, DAY2, |f| f.percepts.iter().any(|p| s.m.g.names[p.0 as usize] == "the campfire"));
        let keys: Vec<Key> = vec![
            (camp + 2, None, "Passing the camp: the campfire is expected there, so it is seen and ignored".into()),
            (FIRE_AT, Some(0), "Fire in the meadow: a big prediction error injects fire and the meadow".into()),
            (FIRE_AT, Some(1), "…and spreads to danger within the same tick".into()),
            (FIRE_AT, None, "Same tick: danger is in the workspace and it flees".into()),
        ];
        let (name, title, form, dec) = if compiled {
            let hd = s.m.compiled.as_ref().unwrap();
            ("meadow-fire-compiled", "Fire in the meadow (compiled form)", "compiled", Some(hd.decompile(&s.m.g.names)))
        } else { ("meadow-fire", "Fire in the meadow, fire at the camp", "graph", None) };
        let sub = if compiled { "The same morning thought with the compiled bit-vector form (2048 bits per node). The links drawn are the compiled form decompiled back to a graph." }
            else { "The morning after the bite, on the way to the berries. The campfire it passes is expected and ignored; a fire where none is expected interrupts within the tick." };
        let html = page(&s, title, sub, form, dec.as_ref(), &keys);
        out(name, html, &keys, &mut frames);
    }

    // One slow graph, four modulator states.
    for (i, (name, h, a, m)) in scenarios::STATES.iter().enumerate() {
        let mut s = scenarios::modulated(SEED, i).recording();
        s.run(24);
        let keys: Vec<Key> = vec![(0, Some(3), format!("{name}: the first think, hop 4")), (8, None, format!("{name}: after 8 ticks"))];
        let file = format!("modulation-{}", name.replace(' ', "-"));
        let html = page(&s, &format!("Same mind, {name}"), &format!("The fox at its den, hunger {h}, arousal held at {a}, mood held at {m}. Same slow graph in every state."), "graph", None, &keys);
        out(&file, html, &keys, &mut frames);
    }

    // Home lifts the mood.
    let mut s = scenarios::mood_at(SEED, "den").recording();
    s.run(80);
    let keys: Vec<Key> = vec![(1, None, "At the den in a low mood".into()), (60, None, "Home: comfort and rest come to mind and the mood lifts".into())];
    let html = page(&s, "Being home", "A fox in a low mood (-500) at its den. The current place is an input: the den brings its feelings back.", "graph", None, &keys);
    out("home-mood", html, &keys, &mut frames);

    // Two temperaments meet the charging dog.
    for (label, id) in [("calm", 26u32), ("quick", 18u32)] {
        let mut s = scenarios::dog_charges(SEED, id).recording();
        s.run(scenarios::CHARGE_AT + 16);
        let t = s.m.temperament;
        let c = scenarios::CHARGE_AT;
        let keys: Vec<Key> = vec![(c, Some(0), format!("{label} fox: the dog charges, hop 1")), (c + 2, None, format!("{label} fox, two ticks in: stress {}", s.frames[(c + 2) as usize].mods.stress))];
        let html = page(&s, &format!("A {label} fox meets the dog"), &format!("Fox #{id}, reactivity {}, aggression {} (seeded). Stress cuts hops and frontier, speeds flight and favours anger.", t.reactivity, t.aggression), "graph", None, &keys);
        out(&format!("temperament-{label}"), html, &keys, &mut frames);
    }
    // A fox that doesn't sleep: bitten on day 1, kept awake through the night.
    let mut s = scenarios::dog_day_no_sleep(SEED).recording();
    s.run(DAY2 + 40);
    let bite = first(&s, 0, |f| f.events.iter().any(|e| e.contains("bites")));
    let keys: Vec<Key> = vec![
        (bite, None, "Bitten: the backlog jumps".into()),
        (bite + 40, None, "Kept awake: idle think budget replays the bite into the slow graph".into()),
        (NIGHT - 2, None, format!("Nightfall: backlog {}, fatigue {} (it can't sleep)", s.frames[(NIGHT - 2) as usize].backlog, s.frames[(NIGHT - 2) as usize].mods.fatigue)),
        (DAY2 + 4, None, "Next morning after a night awake: still tired, still afraid of the henhouse".into()),
    ];
    let html = page(&s, "A fox that doesn't sleep",
        "Day 1 with the dog, then a night lying awake. Idle time still consolidates, so the fear is kept; but the backlog stays, so it stays tired.", "graph", None, &keys);
    out("sleepless", html, &keys, &mut frames);

    // The home experiment: the burrow moved near, out of sight, removed, or doubled.
    for (v, file, title, sub) in [
        (HomeMove::Near, "home-near", "Home moved near", "While the fox is out on day 1, its burrow is moved 9 cells, still in sight of the old den. Day 2 morning: it wakes at the old den."),
        (HomeMove::Mid, "home-mid", "Home moved out of sight", "The burrow is moved 18 cells, out of sight of the old den. Days 3 and 4: searching, then finding it."),
        (HomeMove::Removed, "home-removed", "Home removed", "The burrow is gone. Search, loss, and the old den souring."),
        (HomeMove::Second, "home-second", "A second burrow", "A second, identical-looking burrow appears near the den; the old one stays."),
    ] {
        let days = if v == HomeMove::Mid { 4 } else { 2 };
        let mut s = scenarios::home_moved(SEED, v, days).recording();
        let t0 = if v == HomeMove::Mid { 2 * DAY } else { DAY };
        s.record_from = t0 - 1;
        s.run(days * DAY - 60);
        let shock = first(&s, t0, |f| f.mods.arousal >= 400);
        let search = first(&s, t0, |f| f.why.starts_with("search"));
        let new_seen = first(&s, t0, |f| f.percepts.iter().any(|p| s.m.g.names[p.0 as usize] == "rest" && p.1 >= 400));
        let to_new = first(&s, t0, |f| f.why.contains("new place"));
        let what = if v == HomeMove::Second { "A burrow where none was: a big prediction error" } else { "Waking at the old den: the expected burrow is missing" };
        let mut keys: Vec<Key> = vec![(shock, Some(0), what.into()),
            (shock + 3, None, if v == HomeMove::Second { "Alarm: arousal and stress up; rest and the den fill the workspace" } else { "Alarm: arousal and stress up; the burrow, rest and loss fill the workspace" }.into())];
        if v == HomeMove::Mid || v == HomeMove::Removed { keys.push((search + 6, None, "Unresolved: it searches around the place the burrow went missing".into())); }
        if v == HomeMove::Mid { keys.push((new_seen, None, "Days later, on its rounds: a burrow where none was".into())); }
        if v == HomeMove::Near || v == HomeMove::Mid { keys.push((to_new + 4, None, "Intrigue: it goes to the new burrow".into())); }
        keys.sort_by_key(|k| k.0); keys.dedup_by_key(|k| k.0);
        let html = page(&s, title, sub, "graph", None, &keys);
        out(file, html, &keys, &mut frames);
    }
    let _ = (Hd::compile::<Live>, ());

    frames.truncate(frames.trim_end().len() - 1);
    frames.push_str("\n]\n");
    std::fs::write("viewer/frames.json", frames).unwrap();
}
