//! Deterministic tests: what two (or three) brains learn from talking. World events and starting
//! knowledge are scripted; no line of dialogue is.

use fv_brains_talk::brain::{Brain, Personality};
use fv_brains_talk::world::{Event, World, DAY};
use fv_brains_talk::{Cause, Ent, Fact, Pred, Source, Utt};

const A: u8 = 0;
const B: u8 = 1;
const C: u8 = 2;

fn t(day: u32, tick: u32) -> u32 { (day - 1) * DAY + tick }
fn likes(who: u8, e: Ent) -> Fact { Fact::new(Ent::Agent(who), Pred::Likes, e) }
fn food_in_cold_box() -> Fact { Fact::new(Ent::Food, Pred::IsIn, Ent::ColdBox) }
fn cold_box_holds_food() -> Fact { Fact::new(Ent::ColdBox, Pred::Holds, Ent::Food) }

fn dump(w: &World) {
    for l in &w.trace { println!("{l}") }
    for b in &w.brains { print!("{}", b.describe()) }
}

fn warmth_of_greeting(us: &[Utt]) -> Option<i32> {
    us.iter().find_map(|u| if let Utt::Greet { warmth } = u { Some(*warmth) } else { None })
}

// 1 -------------------------------------------------------------------------------------------

fn introductions(retrospection: bool) -> World {
    let a = Brain::new(A, Personality::WARM).likes(Ent::Flowers).fears(Ent::Dog);
    let mut b = Brain::new(B, Personality::SHY).knows(Fact::new(Ent::Meadow, Pred::Holds, Ent::Flowers));
    b.retrospection = retrospection;
    let mut w = World::new(1, vec![a, b], &[10, 12]);
    w.explain = true;
    w.at(1, 40, Event::Put(B, 90)); // they go their separate ways
    w.run_to(1, 500);
    w
}

#[test]
fn learns_a_fact_about_the_other() {
    let w = introductions(true);
    dump(&w);
    let b = &w.brains[B as usize];
    let like = b.belief(likes(A, Ent::Flowers)).expect("Bo believes Ada likes flowers");
    assert_eq!(like.source, Source::ToldBy(A));
    assert!(like.conf >= 300, "conf {}", like.conf);
    // Derived during retrospection: Ada likes flowers and the meadow holds flowers.
    let derived = b.belief(likes(A, Ent::Meadow)).expect("derived belief");
    assert_eq!(derived.source, Source::Derived);
    // Short-term memory holds nothing about Ada by now.
    assert!(b.short.iter().all(|tr| !tr.involves(A)));
    // Ada believes Bo now knows she likes flowers (theory of mind).
    assert!(w.brains[A as usize].rel_of(B).unwrap().knows.contains(&likes(A, Ent::Flowers)));

    // Control: without retrospection the same conversation leaves nothing behind.
    let w0 = introductions(false);
    let b0 = &w0.brains[B as usize];
    assert!(b0.belief(likes(A, Ent::Flowers)).is_none());
    assert!(b0.short.is_empty(), "short-term decayed to nothing");
    assert!(w0.said_by(A, 0, 40).contains(&Utt::Claim { fact: likes(A, Ent::Flowers), conf: 900 }), "Ada did say it");
}

// 2 -------------------------------------------------------------------------------------------

#[test]
fn teaching_the_same_fact_in_another_form() {
    let a = Brain::new(A, Personality::WARM).knows(food_in_cold_box());
    let b = Brain::new(B, Personality::SHY);
    let mut w = World::new(2, vec![a, b], &[10, 12]);
    w.explain = true;
    w.at(1, 0, Event::Hunger(B, 700));
    w.run_to(1, 100);
    dump(&w);
    let (a, b) = (&w.brains[A as usize], &w.brains[B as usize]);
    // Ada holds it as "food is in the cold box"; Bo was taught "the cold box holds food".
    assert_eq!(a.belief(cold_box_holds_food()).unwrap().fact, food_in_cold_box());
    let taught = w.trace.iter().any(|l| l.contains("Bo    · told Ada: holds(the cold box, food)"));
    assert!(taught);
    // Bo acted on it: walked to the cold box and ate.
    assert_eq!(w.bodies[B as usize].hunger, 0);
    assert!((w.bodies[B as usize].pos - 40).abs() <= 1);
    // Once Bo has looked, the told belief became a seen one, and Ada's credit went up.
    assert_eq!(b.belief(cold_box_holds_food()).unwrap().source, Source::Seen);
    assert!(b.rel_of(A).unwrap().trust > 460);
    // Both would go to the same place via different memory forms before any habit exists.
    let (pa, _, path_a) = a.where_for_food().unwrap();
    assert_eq!(pa, Ent::ColdBox);
    assert!(path_a.contains("food is in X"), "{path_a}");

    // A second Bo who only heard it (and hasn't looked) also heads there, from the told form.
    let a = Brain::new(A, Personality::WARM).knows(food_in_cold_box());
    let mut w2 = World::new(2, vec![a, Brain::new(B, Personality::SHY)], &[10, 12]);
    w2.explain = true;
    w2.at(1, 0, Event::Hunger(B, 700));
    w2.run_to(1, 12);
    let b2 = &w2.brains[B as usize];
    let held = b2.belief(cold_box_holds_food()).unwrap();
    assert_eq!((held.fact, held.source), (cold_box_holds_food(), Source::ToldBy(A)));
    let (pb, _, path_b) = b2.where_for_food().unwrap();
    assert_eq!(pb, Ent::ColdBox);
    assert!(path_b.contains("X holds food"), "{path_b}");

    // After a night, both have compiled it down to the same fast habit.
    w.run_to(2, 1);
    for br in &w.brains { assert_eq!(br.where_for_food().unwrap().2, "habit"); }
}

// 3 -------------------------------------------------------------------------------------------

fn liar_world(lying: bool) -> World {
    let pers = if lying { Personality::SLY } else { Personality::SHY };
    let a = Brain::new(A, pers).knows(food_in_cold_box());
    let b = Brain::new(B, Personality::SHY);
    let mut w = World::new(3, vec![a, b], &[5, 7]);
    w.explain = true;
    // Day 1: both hungry. Day 2: Ada has eaten; Bo is hungry again and they meet again.
    w.at(1, 0, Event::Hunger(A, 800));
    w.at(1, 0, Event::Hunger(B, 700));
    w.at(1, 30, Event::Put(A, 95));
    w.at(2, 50, Event::Hunger(A, 0));
    w.at(2, 50, Event::Hunger(B, 700));
    w.at(2, 50, Event::Put(A, 5));
    w.at(2, 50, Event::Put(B, 7));
    w
}

#[test]
fn trust_falls_after_a_contradicted_claim() {
    let mut w = liar_world(true);
    w.run_to(1, 15);
    let lie = Fact::new(Ent::Well, Pred::Holds, Ent::Food);
    assert!(w.said_by(A, 0, 15).iter().any(|u| matches!(u, Utt::Claim { fact, .. } if *fact == lie)), "the sly, hungry Ada lies");
    let trust_before = w.brains[B as usize].rel_of(A).unwrap().trust;
    w.run_to(1, 200);
    let b = &w.brains[B as usize];
    let trust_after = b.rel_of(A).unwrap().trust;
    assert!((w.bodies[B as usize].pos - 20).abs() <= 1, "Bo went to the well");
    assert!(b.belief(lie).is_none(), "the lie is gone");
    assert_eq!(b.belief(Fact::new(Ent::Well, Pred::Holds, Ent::Water)).unwrap().source, Source::Seen);
    assert!(trust_after * 2 < trust_before, "trust {trust_before} → {trust_after}");

    // Day 2: Ada (fed now) tells the truth, but Bo discounts it and doesn't go.
    w.run_to(2, 200);
    dump(&w);
    assert!(w.said_by(A, t(2, 50), t(2, 200)).iter().any(|u| matches!(u, Utt::Claim { fact, .. } if fact.same(cold_box_holds_food()))));
    let b = &w.brains[B as usize];
    let held = b.belief(cold_box_holds_food()).unwrap();
    assert!(held.conf < fv_brains_talk::brain::ACT_MIN, "conf {}", held.conf);
    assert!(b.where_for_food().is_none());
    assert!(w.bodies[B as usize].hunger > 600, "still hungry");
    assert!(b.episodes.iter().any(|e| e.with == A && e.cause == Cause::Betrayed));

    // Control: an honest Ada is believed on day 2 (Bo ate on day 1, and trusts her more).
    let mut h = liar_world(false);
    h.run_to(2, 200);
    let hb = &h.brains[B as usize];
    assert!(hb.rel_of(A).unwrap().trust > trust_before);
    assert_eq!(h.bodies[B as usize].hunger, 0);
}

// 4 -------------------------------------------------------------------------------------------

fn snarl_world(hurt: bool) -> World {
    let a = Brain::new(A, Personality::PRICKLY).likes(Ent::Water);
    let b = Brain::new(B, Personality::WARM).likes(Ent::Flowers);
    let mut w = World::new(4, vec![a, b], &[10, 12]);
    w.explain = true;
    if hurt { w.at(1, 0, Event::Pain(A, 900)); }
    w.at(1, 40, Event::Put(B, 90));
    w.at(2, 0, Event::Pain(A, 0));
    w.at(4, 100, Event::Put(B, 12));
    w
}

#[test]
fn a_bad_episode_colours_a_later_talk() {
    let mut w = snarl_world(true);
    w.run_to(1, 40);
    assert!(w.said_by(A, 0, 40).iter().any(|u| matches!(u, Utt::Emote { valence, .. } if *valence < 0)), "hurt Ada snarls");
    w.run_to(4, 100);
    {
        let b = &w.brains[B as usize];
        assert!(b.short.iter().all(|tr| !tr.involves(A)), "nothing about Ada left in short-term");
        assert!(b.episodes.iter().any(|e| e.with == A && e.cause == Cause::Snarl));
    }
    w.run_to(4, 200);
    let mut calm = snarl_world(false);
    calm.run_to(4, 200);
    for l in w.trace.iter().filter(|l| l.starts_with("[d4")) { println!("{l}") }
    println!("--- control ---");
    for l in calm.trace.iter().filter(|l| l.starts_with("[d4")) { println!("{l}") }

    let day4 = |w: &World| w.said_by(B, t(4, 100), t(4, 200));
    let stays = |w: &World| w.said.iter().find(|s| s.1 == B && s.0 >= t(4, 100) && s.3 == Utt::Farewell).unwrap().0 - t(4, 100);
    let laughs = |us: &[Utt]| us.iter().filter(|u| matches!(u, Utt::Emote { valence, .. } if *valence > 0)).count();
    let (hurt_talk, calm_talk) = (day4(&w), day4(&calm));
    let (wh, wc) = (warmth_of_greeting(&hurt_talk).unwrap(), warmth_of_greeting(&calm_talk).unwrap());
    let (sh, sc) = (stays(&w), stays(&calm));
    let (lh, lc) = (laughs(&hurt_talk), laughs(&calm_talk));
    println!("Bo on day 4 after the snarl: greeting warmth {wh}, leaves after {sh} ticks, {lh} laughs; without it: {wc}, {sc} ticks, {lc} laughs");
    assert!(sh < sc, "shorter");
    assert!(wh + 300 < wc, "colder greeting");
    assert!(lh < lc, "fewer laughs");
    assert!(w.trace.iter().any(|l| l.starts_with("[d4") && l.contains("recalls Snarl")));
}

// 5 -------------------------------------------------------------------------------------------

#[test]
fn friendship_grows_and_changes_what_is_said() {
    let a = Brain::new(A, Personality::WARM).likes(Ent::Flowers).fears(Ent::Dog);
    let b = Brain::new(B, Personality::SHY).likes(Ent::Flowers);
    let mut w = World::new(5, vec![a, b], &[10, 90]);
    w.explain = true;
    for day in 1..=6 {
        w.at(day, 100, Event::Put(B, 12));
        w.at(day, 160, Event::Put(B, 90));
    }
    let mut fam = vec![];
    let mut aff = vec![];
    for day in 1..=6 {
        w.run_to(day, 500);
        let r = w.brains[B as usize].rel_of(A).unwrap().clone();
        fam.push(r.familiarity);
        aff.push(r.affinity);
    }
    dump(&w);
    println!("Bo→Ada familiarity by day {fam:?}, affinity {aff:?}");
    assert!(fam.windows(2).all(|p| p[1] > p[0]));
    assert!(aff.windows(2).all(|p| p[1] >= p[0]) && aff[5] > aff[0] + 200);
    let first = w.said_by(B, t(1, 0), t(1, 500));
    let last = w.said_by(B, t(6, 0), t(6, 500));
    let introduces = |us: &[Utt]| us.iter().any(|u| matches!(u, Utt::Claim { fact, .. } if fact.s == Ent::Agent(B) && fact.p == Pred::Likes));
    let asks_feels = |us: &[Utt]| us.iter().any(|u| matches!(u, Utt::Ask { p: Pred::Feels, .. }));
    let laughs = |us: &[Utt]| us.iter().filter(|u| matches!(u, Utt::Emote { valence, .. } if *valence > 0)).count();
    assert!(introduces(&first) && !introduces(&last), "strangers introduce themselves; friends know each other");
    assert!(!asks_feels(&first) && asks_feels(&last), "friends ask how the other is");
    assert!(laughs(&last) > laughs(&first));
    assert!(warmth_of_greeting(&last).unwrap() >= 500 && warmth_of_greeting(&first).unwrap() < 500);
    assert!(w.brains[B as usize].rel_of(A).unwrap().shared.contains(&Ent::Flowers));
}

// 6 -------------------------------------------------------------------------------------------

fn three_days(order_flip: bool) -> (u64, Vec<String>) {
    let mut w = liar_world(true);
    w.brains.push(Brain::new(C, Personality::PRICKLY).likes(Ent::Flowers));
    w.bodies.push(w.bodies[0]);
    w.bodies[C as usize].pos = 74;
    w.at(1, 0, Event::Pain(C, 700));
    w.explain = true;
    while w.tick < 3 * DAY {
        if order_flip {
            let mut order: Vec<usize> = (0..w.brains.len()).collect();
            if w.tick % 2 == 0 { order.reverse() } else { order.rotate_left(1) }
            w.step_order(&order);
        } else {
            w.step();
        }
    }
    (w.state_hash(), w.trace.clone())
}

#[test]
fn identical_runs_and_any_brain_order_give_identical_state() {
    let (h1, t1) = three_days(false);
    let (h2, _) = three_days(false);
    let (h3, t3) = three_days(true);
    assert_eq!(h1, h2);
    assert_eq!(h1, h3, "brain update order must not matter");
    assert_eq!(t1, t3);
    println!("state hash after 3 days: {h1:#018x}");
    // Pinned so any machine (or a changed rule) that gives different bits shows up here.
    assert_eq!(h1, PINNED, "update PINNED only when behaviour changes on purpose");
}
const PINNED: u64 = 2541794852942551067;

// 7 -------------------------------------------------------------------------------------------

#[test]
fn quiet_voices_are_missed_or_misheard_deterministically() {
    use fv_brains_talk::sound::{hear, loudness, Heard};
    let u = Utt::Claim { fact: cold_box_holds_food(), conf: 900 };
    assert_eq!(hear(u, loudness(500, 2), 0), Heard::Clear(u));
    assert_eq!(hear(u, loudness(500, 40), 999), Heard::Missed);
    let mid = loudness(500, 18);
    let outcomes: Vec<Heard> = (0..1000).step_by(50).map(|r| hear(u, mid, r)).collect();
    assert!(outcomes.contains(&Heard::Missed) && outcomes.contains(&Heard::Clear(u)));
    let misheard = outcomes.iter().find_map(|h| if let Heard::Misheard(m) = h { Some(*m) } else { None }).unwrap();
    println!("at 18 m (loudness {mid}) «{}» can be heard as «{}»", u.render(0, Some(1)), misheard.render(0, Some(1)));
    assert_eq!(misheard, Utt::Claim { fact: Fact::new(Ent::ColdBox, Pred::Holds, Ent::Flowers), conf: 900 });
}

// 8 -------------------------------------------------------------------------------------------

fn eavesdrop(c_pos: i32) -> World {
    let a = Brain::new(A, Personality::WARM).knows(food_in_cold_box());
    let b = Brain::new(B, Personality::SHY);
    let c = Brain::new(C, Personality::SHY);
    let mut w = World::new(8, vec![a, b, c], &[10, 12, c_pos]);
    w.explain = true;
    w.at(1, 0, Event::Hunger(B, 700));
    w.run_to(1, 60);
    w
}

#[test]
fn a_third_person_overhears() {
    let w = eavesdrop(5);
    dump(&w);
    let c = &w.brains[C as usize];
    let f = c.belief(cold_box_holds_food()).expect("Cal overheard where the food is");
    assert_eq!(f.source, Source::Overheard(A));
    assert!(w.said_by(C, 0, 60).is_empty(), "Cal never spoke");
    assert!(c.rel_of(A).map(|r| r.meetings).unwrap_or(0) == 0, "Cal had no conversation");
    // Bo was told the same thing directly.
    assert_eq!(w.brains[B as usize].belief(cold_box_holds_food()).map(|b| b.source), Some(Source::Seen));

    let far = eavesdrop(-30);
    assert!(far.brains[C as usize].belief(cold_box_holds_food()).is_none(), "out of earshot learns nothing");
}
