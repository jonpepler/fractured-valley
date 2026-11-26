//! Eight days in the valley with Ada, Bo and Cal. World events are scripted (where people are,
//! who is hungry or hurt); every line of dialogue is chosen by a brain.
//!
//!     cargo run --release --example valley_days > trace.txt

use fv_brains_talk::world::{stamp, Event, World, DAY};
use fv_brains_talk::*;

const ADA: u8 = 0;
const BO: u8 = 1;
const CAL: u8 = 2;

fn main() {
    let ada = Brain::new(ADA, Personality::PRICKLY).likes(Ent::Flowers).fears(Ent::Dog).knows(Fact::new(Ent::Food, Pred::IsIn, Ent::ColdBox));
    let bo = Brain::new(BO, Personality::SHY).likes(Ent::Flowers).knows(Fact::new(Ent::Meadow, Pred::Holds, Ent::Flowers));
    let cal = Brain::new(CAL, Personality::SLY).likes(Ent::Water).knows(Fact::new(Ent::Food, Pred::IsIn, Ent::ColdBox));
    let mut w = World::new(2026, vec![ada, bo, cal], &[0, 100, 140]);
    w.explain = true;
    let home = [0, 100, 140];
    let meet = |w: &mut World, day: u32, t: u32, a: u8, b: u8, at: i32| {
        w.at(day, t, Event::Put(a, at));
        w.at(day, t, Event::Put(b, at + 2));
        w.at(day, t + 60, Event::Put(a, home[a as usize]));
        w.at(day, t + 60, Event::Put(b, home[b as usize]));
    };
    // Day 1: Bo wakes hungry and meets Cal, who is hungry too, on the far side of the well.
    w.at(1, 50, Event::Hunger(BO, 700));
    w.at(1, 50, Event::Hunger(CAL, 800));
    w.at(1, 50, Event::Put(BO, 5));
    w.at(1, 50, Event::Put(CAL, 7));
    w.at(1, 70, Event::Put(CAL, home[2]));
    w.at(1, 200, Event::Hunger(CAL, 0));
    // Later Ada, a stranger to Bo, comes to the well.
    w.at(1, 300, Event::Put(ADA, 18));
    w.at(1, 400, Event::Put(ADA, home[0]));
    w.at(1, 500, Event::Put(BO, home[1]));
    // Day 3: Ada has hurt her leg. She meets Bo.
    w.at(3, 0, Event::Pain(ADA, 900));
    meet(&mut w, 3, 100, ADA, BO, 10);
    w.at(4, 0, Event::Pain(ADA, 0));
    // Days 5–8: Ada and Bo meet every morning. On day 7 Bo runs into Cal again, both fed.
    for day in 5..=8 { meet(&mut w, day, 100, ADA, BO, 10); }
    meet(&mut w, 7, 300, CAL, BO, 10);

    let mut shown = 0;
    while w.tick < 8 * DAY + 1 {
        w.step();
        if w.tick % DAY == 801 {
            for l in &w.trace[shown..] { println!("{l}") }
            shown = w.trace.len();
            println!("\n=== memory after day {} ({}) ===", w.tick / DAY + 1, stamp(w.tick));
            for b in &w.brains { print!("{}", b.describe()) }
            println!();
        }
    }
    for l in &w.trace[shown..] { println!("{l}") }
    println!("state hash {:#018x}", w.state_hash());
}
