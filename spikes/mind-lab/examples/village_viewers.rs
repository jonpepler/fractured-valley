//! Viewer pages for stage 5: a day in three villagers' minds (the calmest, the most reactive,
//! and the one who gossips most), and `viewer/village-frames.json` for `tools/frames.cjs`.
//!
//! cargo run --release --example village_viewers && NODE_PATH=$(npm root -g) node tools/frames.cjs out/village-frames viewer/village-frames.json
use fv_mind_lab::fox::viewer::Key;
use fv_mind_lab::people::scenarios::DAY;
use fv_mind_lab::people::viewer::danger_page;
use fv_mind_lab::people::world::name;
use fv_mind_lab::village::runs::{build, Setup};
use std::fmt::Write;

const SEED: u64 = 1;
const N: usize = 30;
const FROM_DAY: u32 = 19;

fn main() {
    std::fs::create_dir_all("viewer").unwrap();
    let s = Setup::new(SEED, N, FROM_DAY + 1);
    let v0 = build(&s);
    let ord: Vec<usize> = (0..N).collect();
    let reac: Vec<i32> = v0.folk.iter().map(|p| p.m.temperament.reactivity).collect();
    let calm = (0..N).min_by_key(|&i| reac[i]).unwrap();
    let jumpy = (0..N).max_by_key(|&i| reac[i]).unwrap();
    // Who gossips most by the start of the recorded day.
    let mut v = build(&s);
    for _ in 0..FROM_DAY * DAY { v.step(1, &ord); v.w.log.clear(); v.w.said.clear(); }
    let talker = (0..N).filter(|&i| i != calm && i != jumpy).max_by_key(|&i| v.folk[i].village.as_ref().unwrap().stats.gossip_said).unwrap();
    let mut list = String::from("[\n");
    for (who, what) in [(calm, "the calmest"), (jumpy, "the most reactive"), (talker, "the keenest gossip")] {
        let mut r = v.clone().recording(who);
        for _ in 0..DAY - 1 { r.step(1, &ord); }
        let fr = &r.frames;
        let me = name(who);
        let find = |f: &dyn Fn(&fv_mind_lab::people::sim::Frame) -> bool| fr.iter().find(|x| f(x)).map(|x| x.tick);
        let mut keys: Vec<Key> = vec![];
        let t0 = fr[0].tick;
        keys.push((t0 + 10, None, format!("{me} ({what}, reactivity {}): morning of day {FROM_DAY}", reac[who])));
        if let Some(t) = find(&|x| x.percepts.iter().any(|p| p.1 >= 400)) { keys.push((t, Some(0), format!("{me} is surprised: a prediction error injects activation and damps the thought on the mind"))); }
        if let Some(t) = find(&|x| x.events.iter().any(|e| e.starts_with(&format!("{me} says")) && !e.contains("good to know") && !e.contains("Watch out"))) { keys.push((t, None, format!("{me} speaks (its own mind chose to)"))); }
        if let Some(t) = find(&|x| x.events.iter().any(|e| e.starts_with(&format!("{me} says")) && (e.contains("good to know") || e.contains("Watch out")))) { keys.push((t, None, format!("{me} gossips: someone it feels strongly about is on its mind"))); }
        if let Some(t) = find(&|x| x.why.contains("danger") || x.why.starts_with("flee") || x.why.starts_with("hold")) { keys.push((t, Some(0), format!("{me} meets danger: the response competition"))); }
        if let Some(t) = find(&|x| x.why.starts_with("eat")) { keys.push((t, None, format!("{me} eats or drinks"))); }
        if let Some(t) = find(&|x| x.tick > t0 + 230) { keys.push((t, None, format!("{me} at dusk: thoughts turn to rest"))); }
        keys.sort_by_key(|k| k.0);
        keys.dedup_by_key(|k| k.0);
        let file = format!("village-{}", ["calm", "reactive", "gossip"][[calm, jumpy, talker].iter().position(|&x| x == who).unwrap()]);
        let html = danger_page(&r, &format!("Village: a day in {me}'s mind ({what})"), &format!("Seed {SEED}, {N} villagers, day {FROM_DAY}. Everything it does comes from its own mind."), &keys);
        std::fs::write(format!("viewer/{file}.html"), html).unwrap();
        for (i, (t, h, label)) in keys.iter().enumerate() {
            writeln!(list, "  {{\"file\": \"{file}.html\", \"t\": {t}, \"h\": {}, \"name\": \"{file}-{}\", \"label\": \"{}\"}},", h.map(|h| h as i64).unwrap_or(-1), i + 1, label.replace('"', "'")).unwrap();
        }
        println!("viewer/{file}.html ({} keys)", keys.len());
    }
    let list = list.trim_end_matches(",\n").to_string() + "\n]\n";
    std::fs::write("viewer/village-frames.json", list).unwrap();
}
