//! Observation streams for scoring tools: what a mind saw, the questions it could ask and the
//! truth, in tick order.
//!
//! - `routine`: a generated valley (hand-written, to have a known structure): people follow a
//!   daily routine between places, with jitter, a 3-day cycle in one stretch of the day, and a
//!   day's random detours. Observers see whoever is within sight.
//! - `village`: a real stage-5 village run (`village::runs`), unchanged; each observer's
//!   sightings are read from its own senses and placed in its own learnt places.
//!
//! Both tell a tool about someone only when the place they are seen at changes or every
//! `RETELL` ticks while in sight (a hand-written input filter, so a tool is not told the same
//! thing every tick).

use super::lang::*;
use crate::rng::hash;

/// While someone stays in sight at one place, tell again every this many ticks.
pub const RETELL: u32 = 10;
/// Ask every this many ticks.
pub const ASK_EVERY: u32 = 10;
pub const DAY: u32 = 301;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Ev {
    /// Observer, tick, record.
    Obs(usize, u32, Rec),
    /// Observer, tick, query (who), the true place.
    Query(usize, u32, Rec, i32),
    /// Night falls at this tick (everyone sleeps).
    Night(u32),
}

#[derive(Clone, Debug, Default)]
pub struct Stream { pub observers: Vec<usize>, pub ticks: u32, pub evs: Vec<Ev>, pub places: usize, pub name: String }
impl Stream {
    pub fn obs_count(&self, o: usize) -> usize { self.evs.iter().filter(|e| matches!(e, Ev::Obs(x, ..) if *x == o)).count() }
}

/// The input filter: (observer, who) → (place last told, tick last told).
#[derive(Default)]
pub struct Teller { last: Vec<((usize, i32), (i32, u32))> }
impl Teller {
    pub fn tell(&mut self, o: usize, who: i32, place: i32, tick: u32) -> bool {
        match self.last.iter_mut().find(|e| e.0 == (o, who)) {
            Some(e) => { if e.1 .0 != place || tick >= e.1 .1 + RETELL { e.1 = (place, tick); true } else { false } }
            None => { self.last.push(((o, who), (place, tick))); true }
        }
    }
}

fn seen(who: usize, place: Option<usize>, x: i32, y: i32) -> Rec {
    rec_of(&[(F::Kind, kind::SEEN), (F::Who, who as i32), (F::Place, place.map_or(NONE, |p| p as i32)), (F::X, x), (F::Y, y)])
}

/// One stretch of a routine: from this tick of the day, be at this place.
#[derive(Clone, Copy, Debug)]
struct Seg { from: u32, place: usize }

/// A generated valley (48 × 32 cells, sight 12): 8 places, `people` people, each with a home and a routine of 5–6
/// stretches a day; the stretch around midday goes somewhere else every third day (a cycle);
/// each stretch starts up to ±10 ticks late or early, and with chance `detour`/1000 a day's
/// stretch goes somewhere random. Observers are `observers`; everyone is a subject.
pub fn routine(seed: u64, people: usize, days: u32, observers: &[usize], detour: u64) -> Stream {
    let (w, h, sight, radius) = (48i32, 32i32, 12i32, 3i32);
    let np = 8usize;
    let r = |a: u64, b: u64, m: u64| hash(seed, a, b, 0x5c4e) % m;
    // Places, at least 9 cells apart.
    let mut places: Vec<(i32, i32)> = vec![];
    let mut k = 0;
    while places.len() < np { let (x, y) = (2 + r(k, 1, (w - 4) as u64) as i32, 2 + r(k, 2, (h - 4) as u64) as i32); k += 1; if places.iter().all(|&(a, b)| (a - x).abs().max((b - y).abs()) >= 9) { places.push((x, y)); } }
    let place_at = |x: i32, y: i32| places.iter().position(|&(a, b)| (a - x).abs().max((b - y).abs()) <= radius);
    // Routines.
    let routines: Vec<(Vec<Seg>, usize)> = (0..people).map(|p| {
        let home = r(p as u64, 10, np as u64) as usize;
        let n = 4 + r(p as u64, 11, 2) as usize;
        let mut segs = vec![Seg { from: 0, place: home }];
        for s in 0..n { let from = 30 + (s as u32 * 220 / n as u32) + r(p as u64, 20 + s as u64, 15) as u32; segs.push(Seg { from, place: r(p as u64, 40 + s as u64, np as u64) as usize }); }
        segs.push(Seg { from: 255, place: home });
        let cycle = r(p as u64, 60, np as u64) as usize;
        (segs, cycle)
    }).collect();
    let mut pos: Vec<(i32, i32)> = routines.iter().map(|(s, _)| places[s[0].place]).collect();
    let mut st = Stream { observers: observers.to_vec(), ticks: days * DAY, places: np, name: format!("routine valley, seed {seed}, {people} people, {days} days"), ..Stream::default() };
    let mut teller = Teller::default();
    for t in 0..days * DAY {
        let (d, tod) = (t / DAY, t % DAY);
        for p in 0..people {
            let (segs, cycle) = &routines[p];
            let mid = segs.len() / 2;
            let mut target = segs[0].place;
            for (si, s) in segs.iter().enumerate() {
                let jit = r(p as u64 * 1000 + si as u64, 100 + d as u64, 21) as i32 - 10;
                if tod as i32 >= s.from as i32 + jit {
                    target = if si == mid && d % 3 == 2 { *cycle } else if r(p as u64 * 1000 + si as u64, 5000 + d as u64, 1000) < detour { r(p as u64 * 1000 + si as u64, 9000 + d as u64, np as u64) as usize } else { s.place };
                }
            }
            let (tx, ty) = places[target];
            let (x, y) = pos[p];
            pos[p] = (x + (tx - x).signum(), y + (ty - y).signum());
        }
        for &o in observers {
            let (ox, oy) = pos[o];
            for p in 0..people {
                if p == o { continue; }
                let (x, y) = pos[p];
                if (x - ox).abs().max((y - oy).abs()) <= sight {
                    let pl = place_at(x, y);
                    if teller.tell(o, p as i32, pl.map_or(NONE, |v| v as i32), t) { st.evs.push(Ev::Obs(o, t, seen(p, pl, x, y))); }
                }
            }
            if t % ASK_EVERY == 0 {
                for p in 0..people { if p == o { continue; } if let Some(pl) = place_at(pos[p].0, pos[p].1) { st.evs.push(Ev::Query(o, t, rec_of(&[(F::Who, p as i32)]), pl as i32)); } }
            }
        }
        if tod == DAY - 1 { st.evs.push(Ev::Night(t)); }
    }
    st
}

/// A real village (`village::runs::build`, unchanged) for `days`. Each observer's sightings of
/// people come from its own senses, placed in its own learnt places; the truth for a question is
/// the observer's own place at the subject's true position (questions where it has none are
/// left out). Subjects are people `0..subjects`.
pub fn village(seed: u64, people: usize, days: u32, observers: &[usize], subjects: usize) -> Stream {
    use crate::people::world::{person_of, Event};
    use crate::village::runs::{build, set_tiers, Setup};
    let s = Setup::new(seed, people, days);
    let mut v = build(&s);
    let ord: Vec<usize> = (0..people).collect();
    let mut st = Stream { observers: observers.to_vec(), ticks: days * DAY, name: format!("village, seed {seed}, {people} people, {days} days"), ..Stream::default() };
    let mut teller = Teller::default();
    for _ in 0..days * DAY {
        let t = v.w.tick;
        set_tiers(&mut v, &s);
        let night = v.w.script.iter().any(|(at, e)| *at == t && *e == Event::Night);
        v.step(1, &ord);
        for &o in observers {
            let m = &v.folk[o].m;
            let sense = v.w.sense(o, false);
            for e in &sense.seen {
                let Some(j) = person_of(e.id, people) else { continue };
                if j >= subjects { continue; }
                let pl = m.place_index_at(e.x, e.y);
                if teller.tell(o, j as i32, pl.map_or(NONE, |p| p as i32), t) { st.evs.push(Ev::Obs(o, t, seen(j, pl, e.x, e.y))); }
            }
            if t % ASK_EVERY == 0 && !v.w.people[o].asleep {
                for j in 0..subjects { if j == o { continue; } let q = &v.w.people[j]; if let Some(pl) = m.place_index_at(q.x, q.y) { st.evs.push(Ev::Query(o, t, rec_of(&[(F::Who, j as i32)]), pl as i32)); } }
            }
            st.places = st.places.max(m.place_nodes.len());
        }
        if night { st.evs.push(Ev::Night(t)); }
    }
    st
}
