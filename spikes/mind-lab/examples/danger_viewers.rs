//! Viewer pages for stage 3 (danger with company), recording Bryn's mind (or Cai's, hearing a cry),
//! and `viewer/danger-frames.json` for `tools/frames.cjs`.
//!
//! cargo run --release --example danger_viewers && NODE_PATH=$(npm root -g) node tools/frames.cjs out/danger viewer/danger-frames.json
use fv_mind_lab::danger::runs::*;
use fv_mind_lab::fox::viewer::Key;
use fv_mind_lab::people::scenarios::{BRYN, CAI};
use fv_mind_lab::people::sim::Valley;
use fv_mind_lab::people::viewer::danger_page;
use std::fmt::Write;

fn first(v: &Valley, from: u32, f: impl Fn(&fv_mind_lab::people::sim::Frame) -> bool) -> u32 { v.frames.iter().find(|x| x.tick >= from && f(x)).map(|x| x.tick).unwrap_or(from) }

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
    let rec = |c: &Case, cast: &Cast, who: usize| -> Valley {
        let (v, _) = build_cast(c, cast);
        let mut v = v.recording(who);
        meet(&mut v, c.seed, 8);
        v.run(WATCH);
        v
    };

    // 1. The hunter, expecting the boar: a small surprise, and the plan holds it braced.
    let v = rec(&Case { seed: 1, temper: TEMPERS[1].1, allies: false, expected: true }, &Cast::default(), BRYN);
    let fight = first(&v, MEET, |f| f.why.starts_with("fight"));
    let keys: Vec<Key> = vec![
        (MEET - 2, None, "Before the meeting: the plan (hunt the boar by the oak) is held in mind, focus full".into()),
        (MEET, Some(0), "The boar appears where the plan said: a small prediction error; thought runs from the plan to standing braced".into()),
        (MEET + 1, None, "Stand my ground wins over fighting; running away never comes into play".into()),
        (fight, None, "As the boar closes and anger builds, fighting overtakes holding".into()),
    ];
    out("danger-hunter", danger_page(&v, "Danger: a hunter meets the boar it came for", "Bryn set out to hunt a boar; its plan predicts one, so the meeting is a small surprise and the plan's own option wins.", &keys), &keys);

    // 2. The forager, alone and taken by surprise: the reflex.
    let v = rec(&Case { seed: 1, temper: TEMPERS[1].1, allies: false, expected: false }, &Cast::default(), BRYN);
    let keys: Vec<Key> = vec![
        (MEET, Some(0), "A boar where none was expected: a full prediction error floods the mind".into()),
        (MEET, None, "Danger passes the flee bar: running away, one hop from danger and made stronger by stress, wins".into()),
        (MEET + 3, None, "Still running; stress climbs and shortens thought".into()),
    ];
    out("danger-forager", danger_page(&v, "Danger: a forager taken by surprise", "Bryn was gathering by the oak with no thought of a boar; the reflex wins.", &keys), &keys);

    // 3. The fierce forager with allies: emboldened, it fights first.
    let v = rec(&Case { seed: 1, temper: TEMPERS[3].1, allies: true, expected: false }, &Cast::default(), BRYN);
    let keys: Vec<Key> = vec![
        (MEET, Some(0), "Surprised, with Cai and Dee beside it: the companions draw attention because Bryn is alarmed".into()),
        (MEET, None, "Through Bryn's own links to 'someone who stands with me', courage lifts fighting over running away".into()),
        (MEET + 3, None, "A few ticks on, stress has shortened thought and flight takes over".into()),
    ];
    out("danger-allies", danger_page(&v, "Danger: emboldened by allies", "A reactive, fierce Bryn, surprised but with two allies close: it turns to fight.", &keys), &keys);

    // 4. Focus under stress: the plan loses control past the point, and the mind improvises.
    let c = Case { seed: 1, temper: TEMPERS[1].1, allies: false, expected: true };
    let (v, _) = build_cast(&c, &Cast::default());
    let mut v = v.recording(BRYN);
    v.beasts.clear();
    meet(&mut v, 1, 6);
    v.folk[BRYN].m.held.pin_stress = Some(700);
    v.run(20);
    let keys: Vec<Key> = vec![
        (MEET, Some(0), "Stress held at 700: rehearsal of the plan is cut, and thought is shortened".into()),
        (MEET + 2, None, "The plan's pull weakens: standing braced loses to fighting (improvising)".into()),
    ];
    out("danger-stress", danger_page(&v, "Danger: a plan under stress", "The same hunter with its stress held at 700 (medium temperament: past its point).", &keys), &keys);

    // 5. A cry for help, heard by Cai out of sight.
    let v = rec(&Case { seed: 1, temper: TEMPERS[1].1, allies: true, expected: false }, &Cast { ally_dist: 16, calls: 700, ..Cast::default() }, CAI);
    let heard = first(&v, MEET, |f| f.events.iter().any(|e| e.starts_with("Bryn says")));
    let aid = first(&v, MEET, |f| f.why.starts_with("aid"));
    let keys: Vec<Key> = vec![
        (heard + 1, Some(0), "Cai hears Bryn cry for help: Bryn, 'a cry for help' and danger come to mind".into()),
        (aid, None, "Going to help wins: Cai walks towards where the cry came from".into()),
        (aid + 4, None, "The cry has brought danger to mind too; later it runs".into()),
    ];
    out("danger-call", danger_page(&v, "Danger: a cry for help", "Bryn, taught to cry out, calls; Cai, 16 cells away and out of sight, hears it.", &keys), &keys);

    list.truncate(list.len() - 2);
    list.push_str("\n]\n");
    std::fs::write("viewer/danger-frames.json", list).unwrap();
}
