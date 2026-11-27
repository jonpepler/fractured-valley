//! Writes viewer pages for the fox scenarios on the shared mind into `viewer/`, and
//! `viewer/frames.json`, the key moments `tools/frames.cjs` turns into pictures. The pages are
//! drawn from the mind's expanded form; the mind ran on its runtime form.
//!
//! cargo run --release --example viewers && NODE_PATH=$(npm root -g) node tools/frames.cjs
use fv_mind_lab::fox::scenarios::{self, HomeMove, DAY, DAY2, NIGHT, SEED};
use fv_mind_lab::fox::sim::{Frame, Sim};
use fv_mind_lab::fox::viewer::{page, Key};
use fv_mind_lab::mind::Action;
use std::fmt::Write;

fn first(s: &Sim, from: u32, f: impl Fn(&Frame) -> bool) -> u32 { s.frames.iter().find(|x| x.tick >= from && f(x)).map(|x| x.tick).unwrap_or(from) }

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

    let mut s = scenarios::dog_day(SEED).recording();
    s.run(DAY2 + 40);
    let approach = first(&s, 0, |f| f.percepts.iter().any(|p| p.1 >= 400));
    let bite = first(&s, 0, |f| f.events.iter().any(|e| e.contains("bites")));
    let berries = first(&s, bite, |f| f.why.contains("berry bush"));
    let nap = first(&s, bite, |f| f.why.starts_with("tired"));
    let eats = first(&s, bite, |f| matches!(f.action, Action::Eat { .. }));
    let keys: Vec<Key> = vec![
        (2, Some(1), "Shared mind: hunger spreads to food, the hens and the berries".into()),
        (approach, None, "The dog charges: a prediction error interrupts the thought".into()),
        (bite, None, "Bitten: pain and danger fill the workspace; it flees".into()),
        (berries, None, "The henhouse now feels bad; it turns to the berry bush".into()),
        (nap, None, "Tired from its backlog, it naps at the den and consolidates".into()),
        (eats, None, "It eats at the berry bush".into()),
        (DAY2 + 4, None, "Next morning: no fast weights left, but the henhouse still feels bad".into()),
    ];
    let html = page(&s, "Shared mind: a bad day at the henhouse, and a night's sleep",
        "The fox on the shared mind (species base + overlay). Day 1: bitten at the henhouse. Night: consolidation into the overlay. Day 2: the henhouse carries the fear.", &keys);
    out("dog-and-sleep", html, &keys, &mut frames);

    let mut s = scenarios::meadow_fire(SEED).recording();
    s.record_from = NIGHT + 1;
    s.run(DAY2 + 80);
    let camp = first(&s, DAY2, |f| f.percepts.iter().any(|p| s.m.name(p.0) == "the campfire"));
    let keys: Vec<Key> = vec![
        (camp + 2, None, "Passing the camp: the campfire is expected there, so it is ignored".into()),
        (scenarios::FIRE_AT, Some(0), "Fire in the meadow: a prediction error of 1000".into()),
        (scenarios::FIRE_AT, None, "Danger on the mind and a bad thing in sight: it flees in the same tick".into()),
    ];
    let html = page(&s, "Shared mind: fire in the meadow", "Day 2. The expected campfire is ignored; the unexpected meadow fire interrupts within a tick.", &keys);
    out("meadow-fire", html, &keys, &mut frames);

    let mut s = scenarios::home_moved(SEED, HomeMove::Near, 2).recording();
    s.record_from = DAY - 1;
    s.run(2 * DAY - 60);
    let shock = first(&s, DAY, |f| f.mods.arousal >= 400);
    let to_new = first(&s, DAY, |f| f.why.contains("new place"));
    let keys: Vec<Key> = vec![
        (shock, Some(0), "Waking at the old den: the expected burrow is missing".into()),
        (shock + 3, None, "Alarm: the burrow, rest and loss fill the workspace".into()),
        (to_new + 4, None, "Intrigue: it goes to the new burrow".into()),
    ];
    let html = page(&s, "Shared mind: home moved near", "While the fox is out on day 1, its burrow is moved 9 cells, in sight of the old den. Day 2: it wakes at the old den.", &keys);
    out("home-near", html, &keys, &mut frames);

    frames.truncate(frames.trim_end().len() - 1);
    frames.push_str("\n]\n");
    std::fs::write("viewer/frames.json", frames).unwrap();
}
