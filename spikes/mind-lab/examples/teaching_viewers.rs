//! Viewer pages for key teaching moments, recording Bryn's mind (or Cai's), and
//! `viewer/teaching-frames.json` for `tools/frames.cjs`.
//!
//! cargo run --release --example teaching_viewers && NODE_PATH=$(npm root -g) node tools/frames.cjs out/teaching viewer/teaching-frames.json
use fv_mind_lab::fox::viewer::Key;
use fv_mind_lab::people::person::Medium;
use fv_mind_lab::people::scenarios::*;
use fv_mind_lab::people::sim::{Frame, Valley};
use fv_mind_lab::people::viewer::page;
use fv_mind_lab::people::world::Event;
use std::fmt::Write;

fn first(v: &Valley, from: u32, f: impl Fn(&Frame) -> bool) -> u32 { v.frames.iter().find(|x| x.tick >= from && f(x)).map(|x| x.tick).unwrap_or(from) }

fn main() {
    std::fs::create_dir_all("viewer").unwrap();
    let mut list = String::from("[\n");
    let mut out = |file: &str, html: String, keys: &[Key]| {
        std::fs::write(format!("viewer/{file}.html"), html).unwrap();
        for (i, (t, h, label)) in keys.iter().enumerate() {
            writeln!(list, "  {{\"file\": \"{file}.html\", \"t\": {t}, \"h\": {}, \"name\": \"{file}-{}\", \"label\": \"{label}\"}},", h.map(|h| h as i64).unwrap_or(-1), i + 1).unwrap();
        }
        println!("viewer/{file}.html");
    };

    // 1–2. Told, goes, finds it.
    let mut s = Setup::new(1); s.days = 1;
    let mut v = s.build().recording(BRYN);
    v.run(70);
    let heard = first(&v, 0, |f| f.events.iter().any(|e| e.starts_with("Ash says")));
    let found = bryn_told(&v).and_then(|t| t.confirmed).unwrap_or(30);
    let drink = first(&v, found, |f| f.events.iter().any(|e| e.contains("Bryn drinks")));
    let keys: Vec<Key> = vec![
        (heard + 1, Some(0), "Bryn hears Ash: the words ground in Bryn's own landmarks (rock, windmill)".into()),
        (heard + 2, None, "Thirst runs to water and on to the told spring; Ash, the source, is on the mind".into()),
        (found, None, "Water seen where it was told: the tentative place is moved onto the real spring".into()),
        (drink, None, "Bryn drinks at the spring".into()),
    ];
    let html = page(&v, "Teaching: told of a spring, Bryn finds it", "Ash describes the spring by landmarks; Bryn builds its own nodes for it, with a source link to Ash, and goes.", &keys);
    out("teaching-told", html, &keys);

    // 2. Dry: a surprise, and the source link.
    let mut s = Setup::new(1); s.days = 3; s.spring = Spring::DriesAt(5);
    let mut v = s.build().recording(BRYN);
    let (x, y) = v.w.spring();
    v.at(DAY + 20, Event::Put { who: ASH, x: x + 2, y: y + 2 });
    v.run(3 * DAY);
    let dry = bryn_told(&v).and_then(|t| t.dry).unwrap_or(30);
    let corr = first(&v, DAY, |f| f.events.iter().any(|e| e.starts_with("Ash says to Bryn: Careful")));
    let keys: Vec<Key> = vec![
        (dry, Some(0), "The spring is dry: an expected thing missing, and a dry hollow where water should be".into()),
        (dry + 1, None, "Surprise and loss; the told water and its place fill the workspace".into()),
        (corr + 1, None, "Ash, having seen it dry, warns Bryn: the episode is judged a mistake".into()),
    ];
    let html = page(&v, "Teaching: the spring is dry", "The spring dries just after Ash tells Bryn of it. Bryn finds a fresh, dry hollow; later Ash corrects itself.", &keys);
    out("teaching-dry", html, &keys);

    // 6. A lost note found by chance.
    let mut s = Setup::new(1); s.days = 4; s.medium = Medium::Note;
    let mut v = s.build().recording(BRYN);
    v.at(4, Event::SetThirst { who: BRYN, v: 0 });
    for t in (10..4 * DAY).step_by(50) { v.at(t, Event::SetThirst { who: BRYN, v: 0 }); }
    v.at(5, Event::Away { who: ASH });
    v.at(DAY + 10, Event::LoseNotes { who: BRYN });
    v.at(DAY + 11, Event::MoveNote { id: 100, x: 62, y: 1 });
    v.run_until(3 * DAY + 5);
    let (bx, by) = (v.w.people[BRYN].x, v.w.people[BRYN].y);
    let t0 = v.w.tick;
    v.at(t0, Event::MoveNote { id: 100, x: bx + 3, y: by });
    v.at(t0 + 1, Event::MoveNote { id: 100, x: bx, y: by });
    v.run(6);
    let keys: Vec<Key> = vec![
        (2, None, "Ash writes the claim on a note: Bryn reads it; the note has it, the fact is held less".into()),
        (t0, Some(0), "Days later, Bryn sees its lost note: the note's node lights, and the fact with it".into()),
        (t0 + 2, None, "Picked up and reread: the faded fact and source come back".into()),
    ];
    let html = page(&v, "Teaching: a note, lost and found", "Ash writes; Bryn keeps the note, loses it on day 1, and comes on it again on day 3.", &keys);
    out("teaching-note", html, &keys);

    // 4. Passed on: Cai's mind.
    let mut s = Setup::new(1); s.days = 3; s.pos[CAI] = (24, 6);
    let mut v = s.build().recording(CAI);
    v.w.people[CAI].thirst = 0;
    v.at(DAY + 2, Event::Put { who: CAI, x: 13, y: 24 });
    v.at(DAY + 2, Event::Put { who: BRYN, x: 12, y: 23 });
    v.at(DAY + 2, Event::SetThirst { who: CAI, v: 650 });
    v.run_until(DAY + 80);
    let told = first(&v, DAY, |f| f.events.iter().any(|e| e.starts_with("Bryn says to Cai")));
    let keys: Vec<Key> = vec![(told + 1, None, "Cai hears Bryn, who says Ash said: two source links, one to Bryn and one to Ash".into())];
    let html = page(&v, "Teaching: passed on", "Bryn, having found the spring, tells Cai; Cai grounds it in its own landmarks (it doesn't know the rock).", &keys);
    out("teaching-chain", html, &keys);

    list.truncate(list.trim_end().len() - 1);
    list.push_str("\n]\n");
    std::fs::write("viewer/teaching-frames.json", list).unwrap();
}
