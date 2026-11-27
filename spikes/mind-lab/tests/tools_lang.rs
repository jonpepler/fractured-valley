//! The tool language: every kit tool parses, checks, round-trips through text and bytes, and the
//! interpreter does what each cell says.
use fv_mind_lab::tools::kit;
use fv_mind_lab::tools::lang::*;
use fv_mind_lab::tools::run::{Fuel, Tool};

fn all() -> Vec<&'static str> { let mut v: Vec<&str> = kit::FOUNDATION.to_vec(); v.extend(kit::SCHEDULES); v.push(kit::MAKER); v.push(kit::FRUGAL_MAKER); v }

#[test]
fn every_kit_tool_round_trips() {
    for src in all() {
        let d = kit::load(src);
        let text = d.to_string();
        let d2 = parse(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
        assert_eq!(d, d2, "text round trip\n{text}");
        let b = d.encode();
        assert_eq!(Desc::decode(&b).unwrap(), d, "binary round trip");
        assert_eq!(d.digest(), d2.digest());
        println!("{:>18}: {:>3} bytes, shape {:016x}\n  {}", d.name, d.size(), d.shape(), d.explain());
    }
}

#[test]
fn checking_refuses_bad_tools() {
    for bad in [
        r#"(tool "x" (slot t (key entity) (votes 3) place 8) (ask (top t (who tod) 3)))"#, // wrong key length
        r#"(tool "x" (slot t (key entity) (votes 3) place 8) (ask (top t (place) 3)))"#,   // wrong key type
        r#"(tool "x" (slot t (key entity) (votes 9) place 8) (ask (top t (who) 3)))"#,      // too many votes
        r#"(tool "x" (slot t (key entity) (votes 3) place 8) (observe (put t (who) who 1)) (ask (top t (who) 3)))"#, // value type
        r#"(tool "x" (slot t (key entity) (votes 3) place 8) (ask (top u (who) 3)))"#,      // no slot
    ] { assert!(parse(bad).is_err(), "should refuse {bad}"); }
    // Garbage bytes never panic.
    let d = kit::load(kit::TIMETABLE).encode();
    for i in 0..d.len() { for x in [0u8, 1, 0x7f, 0xff] { let mut b = d.clone(); b[i] = x; let _ = Desc::decode(&b); } let _ = Desc::decode(&d[..i]); }
}

fn seen(tick: i32, who: i32, place: i32) -> Rec { let mut r = rec_of(&[(F::Kind, kind::SEEN), (F::Who, who), (F::Place, place)]); fv_mind_lab::tools::belt::stamp(&mut r, tick as u32, 301); r }

#[test]
fn timetable_votes_and_fades() {
    let mut t = Tool::from(kit::load(kit::TIMETABLE));
    for d in 0..3 { t.observe(&seen(d * 301 + 10, 0, 4), 1000).unwrap(); t.observe(&seen(d * 301 + 12, 0, 5), 1000).unwrap(); }
    t.observe(&seen(1000 + 14, 0, 5), 1000).unwrap();
    let q = { let mut q = rec_of(&[(F::Who, 0)]); fv_mind_lab::tools::belt::stamp(&mut q, 11, 301); q };
    let (a, used) = t.ask(&q, 100);
    println!("answer {:?}, {used} fuel", a.as_ref().unwrap());
    assert_eq!(a.unwrap().vals()[0], (4, 3));
    // Out of fuel says so.
    assert!(t.ask(&q, 2).0.is_err());
    for _ in 0..12 { t.forget(0, 1000).unwrap(); }
    assert_eq!(t.entries(), 0, "fading empties the table");
}

#[test]
fn sequence_follows_the_route() {
    let mut t = Tool::from(kit::load(kit::SEQUENCE));
    // Ash goes 1 → 2 → 3 → 1, staying 20 ticks at each, for 5 rounds.
    let mut tick = 0;
    for _ in 0..5 { for p in [1, 2, 3] { for k in 0..2 { t.observe(&seen(tick + k * 10, 0, p), 1000).unwrap(); } tick += 20; } }
    // Last seen at 3 (arrived at `tick - 20`): 25 ticks later it should be at 1; 45 later at 2.
    let last = tick - 20;
    let mut f = Fuel::new(500);
    let at = |dt: i32, f: &mut Fuel| { let mut q = rec_of(&[(F::Who, 0)]); fv_mind_lab::tools::belt::stamp(&mut q, (last + dt) as u32, 301); t.answer_only(&q, f).unwrap()[0].0 };
    assert_eq!(at(5, &mut f), 3); assert_eq!(at(25, &mut f), 1); assert_eq!(at(45, &mut f), 2);
}

#[test]
fn a_path_over_a_map() {
    let src = r#"(tool "m" (slot map (key place) (votes 4) place 64)
      (observe (if (= kind moved) (put map (place) next (if (< val 0) -32 1)) (put map (next) place (if (< val 0) -32 1))))
      (ask (path map place goal 16)))"#;
    let mut t = Tool::from(parse(src).unwrap());
    let mv = |a: i32, b: i32, v: i32| rec_of(&[(F::Kind, kind::MOVED), (F::Place, a), (F::Next, b), (F::Val, v)]);
    for (a, b) in [(0, 1), (1, 2), (2, 3), (3, 4), (0, 5), (5, 4)] { t.observe(&mv(a, b, NONE), 1000).unwrap(); }
    let q = |a: i32, b: i32| rec_of(&[(F::Place, a), (F::Goal, b)]);
    let mut f = Fuel::new(10_000);
    assert_eq!(t.answer_only(&q(0, 4), &mut f).unwrap()[0], (5, 980), "two steps by 5");
    t.observe(&mv(0, 5, -1000), 1000).unwrap(); // blocked
    assert_eq!(t.answer_only(&q(0, 4), &mut f).unwrap()[0].0, 1, "round by 1 once 0–5 is blocked");
    assert_eq!(t.answer_only(&q(4, 0), &mut f).unwrap()[0].0, 3);
}
