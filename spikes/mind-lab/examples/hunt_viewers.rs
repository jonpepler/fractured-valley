//! Viewer pages for stage 4 (the hunt and the betrayal), and `viewer/hunt-frames.json` for
//! `tools/frames.cjs`.
//!
//! cargo run --release --example hunt_viewers && NODE_PATH=$(npm root -g) node tools/frames.cjs out/hunt viewer/hunt-frames.json
use fv_mind_lab::fox::viewer::Key;
use fv_mind_lab::hunt::runs::*;
use fv_mind_lab::people::scenarios::{ASH, BRYN, CAI, DEE};
use fv_mind_lab::people::sim::Valley;
use fv_mind_lab::people::viewer::danger_page;
use std::fmt::Write;

fn ev(v: &Valley, who: usize, pat: &str) -> Option<u32> { v.folk[who].hunt.as_ref().unwrap().log.events.iter().find(|e| e.1.contains(pat)).map(|e| e.0) }

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
    let names = ["ash", "bryn", "cai", "dee"];
    // 1. The plan forming in each mind (seed 7: everyone joins), and one who declines (seed 1, Cai).
    for who in [ASH, BRYN, DEE] {
        let (_, v) = hunt_rec(&Case { seed: 7, set: 1, mode: Mode::Planned }, Some(who));
        let t = if who == ASH { 0 } else { 1 };
        let keys: Vec<Key> = vec![(t, None, if who == ASH { "Ash composes the plan from its own map and proposes it: its plan node links the deer, its spot, its role, each member".into() } else { format!("{} hears the plan, grounds the places in its own landmarks, and builds a plan node of its own: it joins", ["Ash", "Bryn", "Cai", "Dee"][who]) }),
            (t + 6, None, "On the way to its spot, the plan is held in mind (rehearsed) and predicts where the others will be".into())];
        out(&format!("hunt-plan-{}", names[who]), danger_page(&v, &format!("Hunt: the plan in {}'s mind", ["Ash", "Bryn", "Cai", "Dee"][who]), "Seed 7: Ash proposes a drive and an ambush by the rock; each hearer decides for itself.", &keys), &keys);
    }
    let (_, v) = hunt_rec(&Case { seed: 1, set: 1, mode: Mode::Planned }, Some(CAI));
    let keys: Vec<Key> = vec![(1, None, "Cai hears the plan; it feels cool towards Ash and trusts it little, so the plan feels bad to it: it declines out loud".into())];
    out("hunt-plan-cai-declines", danger_page(&v, "Hunt: a hearer declines", "Seed 1: Cai's own feeling for the plan (warmth and trust towards Ash) is below the bar.", &keys), &keys);
    // 2. The breach: Ash sees the deer move before the signal (seed 7).
    let (_, v) = hunt_rec(&Case { seed: 7, set: 1, mode: Mode::Planned }, Some(ASH));
    let b = ev(&v, ASH, "moved early").unwrap_or(22);
    let keys: Vec<Key> = vec![(b - 1, None, "Waiting by the rock: the plan predicts the deer grazing at its meadow".into()),
        (b, Some(0), "The deer is not where the plan said: a prediction error; 'a broken plan' and danger light up; Ash whistles at once".into()),
        (b + 2, None, "After the breach: arousal up, the plan's hold shaken; the drivers set off on the whistle".into())];
    out("hunt-breach", danger_page(&v, "Hunt: the plan breaks", "Seed 7: the deer, disturbed by the hunters walking to their spots, moves before the signal.", &keys), &keys);
    // 3. The betrayal (seed 1, Cai leans to Dee).
    let c = BCase { seed: 1, set: 1, lean: Lean::Cai, caution: 10, honest_teller: false };
    let (_, v) = betrayal_rec(&c, Some(CAI));
    let w = ev(&v, CAI, "whispers").unwrap_or(40);
    let keys: Vec<Key> = vec![(1, None, "Cai hears the plan against Dee. It feels more for Dee than for Ash: it says 'I'm in', and forms an intent of its own, to tell Dee".into()),
        (w, None, "Out of the others' sight, Cai whispers the plan to Dee".into())];
    out("hunt-betray-cai", danger_page(&v, "Betrayal: the one who tells", "Cai likes Dee and owes it; its own feelings, not a script, choose it.", &keys), &keys);
    let (_, v) = betrayal_rec(&c, Some(DEE));
    let keys: Vec<Key> = vec![(w + 1, None, "Dee is warned: their plan becomes predictions of its own (who waits where), and it proposes a counter-plan to Cai, in a whisper".into()),
        (w + 4, None, "Dee and Cai set off for a spot behind the ones lying in wait".into())];
    out("hunt-betray-dee", danger_page(&v, "Betrayal: the warned", "Dee grounds the group's plan in its own landmarks and plans to turn it round.", &keys), &keys);
    let (_, v) = betrayal_rec(&c, Some(ASH));
    let br = v.folk[ASH].hunt.as_ref().unwrap().log.reactions.iter().map(|r| r.0).find(|&t| t > w).unwrap_or(w + 6);
    let keys: Vec<Key> = vec![(br - 1, None, "Ash waits hidden by the windmill, predicting Dee on its way and Cai at its post".into()),
        (br, Some(0), "The breach: someone is not where the plan said: surprise, a broken plan, danger".into()),
        (br + 3, None, "The ambush turned into a fight; afterwards Ash weighs who could have told".into())];
    out("hunt-betray-ash", danger_page(&v, "Betrayal: the breach", "The group's leader experiences the breach as surprise and danger.", &keys), &keys);
    list.truncate(list.len() - 2);
    list.push_str("\n]\n");
    std::fs::write("viewer/hunt-frames.json", list).unwrap();
}
