//! Storage and cost of teaching: what a told fact, a source link, a note and a map cost a mind;
//! how source links pile up over a simulated year of occasional telling (with and without
//! forgetting); and µs per person-tick, per grounding, per night.
//! `cargo test --release --test teaching_storage -- --nocapture --test-threads=1`

use fv_mind_lab::graph::lt;
use fv_mind_lab::people::person::{Medium, Person, Told};
use fv_mind_lab::people::scenarios::{Setup, ASH, BRYN, CAI, DAY, DEE};
use fv_mind_lab::people::talk::{band_dir, Content, Drawn, Ref};
use fv_mind_lab::people::world::{Note, ROCK, WINDMILL};
use fv_mind_lab::rng::hash;
use fv_mind_lab::store::Store;
use fv_mind_lab::think::Form;
use std::time::Instant;

fn own(p: &Person) -> usize { p.m.store.bytes().own }
fn links(p: &Person) -> usize { p.m.store.ov.added.len() }

#[test]
fn what_teaching_costs_a_mind() {
    for (name, medium) in [("spoken", Medium::Speak), ("written", Medium::Note), ("drawn map", Medium::Map)] {
        let mut s = Setup::new(1); s.medium = medium;
        let mut v = s.build();
        let (b0, l0, n0) = (own(&v.folk[BRYN]), links(&v.folk[BRYN]), v.folk[BRYN].m.store.len());
        v.run(3);
        let b = &v.folk[BRYN];
        let src = b.m.store.ov.added.iter().filter(|a| a.ty == lt::SOURCE).count();
        let note = v.w.notes.first().map(|n| std::mem::size_of::<Note>() + n.content.refs.len() * std::mem::size_of::<Ref>() + n.content.drawn.len() * std::mem::size_of::<Drawn>()).unwrap_or(0);
        let drawn = v.w.notes.first().map(|n| n.content.drawn.len()).unwrap_or(0);
        println!("{name:10}: +{} B in Bryn's own mind ({} nodes, {} links, of which {src} source links at 16 B each); the told record (people layer) {} B; the note itself in the world {note} B ({drawn} drawn landmarks)",
            own(b) - b0, b.m.store.len() - n0, links(b) - l0, std::mem::size_of::<Told>());
    }
    // Passed on: Cai's node for the spring has a source link to Bryn and one to Ash ("Bryn said Ash said").
    println!("a link costs 16 B as held (an added link: source, target, type, weight, valence); a node of the mind's own 5 B + 2 B valence + its anchor (12 B)");
}

/// One mind told a new place every 3 days for a year, by rotating tellers, never using any of
/// them (so nothing rehearses them): how many source links does it hold, and how many bytes?
fn year(fade: bool) -> (Vec<(u32, usize, usize, usize)>, usize) {
    let mut v = Setup::new(1).build();
    let mut p = v.folk[BRYN].clone();
    if !fade { p.m.cfg.own_fade = [0; lt::COUNT]; }
    let w = v.w.clone();
    let mut out = vec![];
    let (rx, ry) = (w.thing(ROCK).x, w.thing(ROCK).y);
    let (wx, wy) = (w.thing(WINDMILL).x, w.thing(WINDMILL).y);
    for day in 0..365u32 {
        if day % 3 == 0 {
            let (x, y) = (4 + (hash(7, day as u64, 1, 0) % 56) as i32, 3 + (hash(7, day as u64, 2, 0) % 34) as i32);
            let refs = [(ROCK, rx, ry), (WINDMILL, wx, wy)].iter().map(|&(l, lx, ly)| { let (band, dir) = band_dir(x - lx, y - ly); Ref { landmark: l, dir, band } }).collect();
            let from = [ASH, CAI, DEE][(day / 3 % 3) as usize];
            let mut cues = vec![];
            p.ground(&Content { refs, drawn: vec![], conf: 900, via: None, dry: false }, Some(from), false, None, day * DAY, &mut cues);
        }
        p.m.sleep();
        if day % 30 == 29 || day == 364 {
            let src = p.m.store.ov.added.iter().filter(|a| a.ty == lt::SOURCE).count();
            out.push((day + 1, src, links(&p), own(&p)));
        }
    }
    let _ = &mut v;
    let places: std::collections::BTreeSet<usize> = p.told.iter().map(|t| t.place).collect();
    (out, places.len())
}

#[test]
fn source_links_over_a_year_of_occasional_telling() {
    for fade in [true, false] {
        let (y, _) = year(fade);
        println!("{}: (day, source links, own links, own bytes) {:?}", if fade { "with forgetting" } else { "no forgetting  " }, y);
    }
    let ((with, places), (without, _)) = (year(true), year(false));
    let (a, b) = (with.last().unwrap(), without.last().unwrap());
    println!("after a year (122 tellings, landing on {places} distinct places: claims near an earlier one join it): {} source links with forgetting against {} without; own mind {} B against {} B", a.1, b.1, a.3, b.3);
    assert!(a.1 < b.1, "forgetting removes faded source links");
}

#[test]
fn what_teaching_costs_in_time() {
    // A day of four people (talk, a confirmed spring, thanks), stepped on one thread.
    let mut best = f64::MAX;
    for _ in 0..5 {
        let mut v = Setup::new(1).build();
        let t = Instant::now();
        v.run(DAY);
        best = best.min(t.elapsed().as_secs_f64());
    }
    let per = best * 1e6 / (DAY as f64 * 4.0);
    println!("a day of 4 people: {:.1} ms, {per:.1} µs per person-tick (with nights' sleep and talk)", best * 1e3);
    // Grounding one spoken claim, a map and a night's sleep with forgetting.
    let v = Setup::new(1).build();
    let w = v.w.clone();
    let (rx, ry) = (w.thing(ROCK).x, w.thing(ROCK).y);
    let c = Content { refs: vec![{ let (band, dir) = band_dir(9, -3); Ref { landmark: ROCK, dir, band } }], drawn: vec![], conf: 900, via: None, dry: false };
    let m = Content { refs: vec![], drawn: vec![Drawn { landmark: ROCK, dx: -4, dy: 2 }, Drawn { landmark: WINDMILL, dx: 3, dy: 5 }], conf: 900, via: None, dry: false };
    for (name, content) in [("a spoken claim", &c), ("a map", &m)] {
        let mut best = f64::MAX;
        for _ in 0..200 { let mut p = v.folk[BRYN].clone(); let mut cues = vec![]; let t = Instant::now(); p.ground(content, Some(ASH), false, None, 0, &mut cues); best = best.min(t.elapsed().as_secs_f64()); }
        println!("grounding {name}: {:.2} µs", best * 1e6);
    }
    let mut p = v.folk[BRYN].clone(); let mut cues = vec![];
    p.ground(&c, Some(ASH), false, None, 0, &mut cues);
    let mut best = f64::MAX;
    for _ in 0..50 { let mut q = p.clone(); let t = Instant::now(); q.m.sleep(); best = best.min(t.elapsed().as_secs_f64()); }
    println!("a night's sleep with forgetting (one told fact held): {:.1} µs", best * 1e6);
    let _ = (rx, ry);
}
