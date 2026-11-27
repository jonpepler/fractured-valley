//! The brain-tick demo: Bryn learns where Ash goes, builds a tool for it in sleep, and from then
//! on finds Ash by asking the tool rather than thinking it through or wandering.
//!
//! A real stage-5 village (`village::runs::build`, unchanged; nothing here writes to it). Bryn
//! (person 1) has a belt, on. Every tick Bryn's own sightings of Ash go to the belt, placed in
//! Bryn's own learnt places. Four times a day (hashed times, while awake) Bryn wants Ash:
//!
//! - **Thinking it through** (always computed, for comparison): Bryn's real mind imagines Ash
//!   (`Mind::imagine`, Ash's node cued, three thinks); its learnt places ranked by activation are
//!   where it would look, in order. Cost: the links it read.
//! - **The brain-tick hook**, once a tool is bound: a clone of Bryn's mind takes a real step with
//!   Ash cued (as wanting Ash would bring Ash to mind); if Ash's node is in its workspace, the
//!   belt's binding fires (`Belt::tick`), and the answer comes back as cues on Bryn's place
//!   nodes; a second step on the clone takes them in, and we check the answer came to mind (is
//!   in the workspace). The clone is thrown away: the village is untouched.
//! - Until a tool exists, each question is logged as thought through (`Belt::thought`), with
//!   its cost; each night the tool-maker may build one.
//!
//! **Time to find Ash**, worked out afterwards against the recorded run: from where Bryn stood,
//! walk one cell a tick to each place in the strategy's order, then to the rest of Bryn's places
//! nearest first; found when Ash (where Ash really was that tick) is within sight. Strategies:
//! wander (nearest first), think, tool. The walk is hand-written; the orders are the mind's or
//! the tool's.

use super::belt::{Belt, Reach, ToolConfig};
use super::cost::Brain;
use super::data::Teller;
use super::kit;
use super::lang::*;
use super::study::where_is;
use crate::people::world::{person_of, Event, SIGHT};
use crate::rng::hash;
use crate::think::Form;
use crate::village::runs::{build, layout, set_tiers, Setup};

pub const ASH: usize = 0;
pub const BRYN: usize = 1;
pub const DAY: u32 = 301;
/// Give up looking after this many ticks.
pub const GIVE_UP: u32 = 300;

#[derive(Clone, Debug, Default)]
pub struct Question {
    pub tick: u32,
    pub from: (i32, i32),
    /// Bryn's learnt places then: (index, x, y).
    pub places: Vec<(usize, i32, i32)>,
    pub think: Vec<usize>, pub think_cost: u32, pub think_ns: u64,
    pub tool: Option<Vec<usize>>, pub tool_fuel: i64, pub tool_ns: u64,
    /// Did the tool's answer come to mind on the clone's next step (in its workspace)?
    pub came_to_mind: Option<bool>,
    /// Was Ash's node in the clone's workspace, so the binding fired?
    pub fired: bool,
    /// The clone's two steps, in link reads (what the hook costs the mind beyond the tool).
    pub step_visits: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Demo { pub qs: Vec<Question>, pub ash: Vec<(i32, i32)>, pub made: Vec<(u32, String, String)>, pub sightings: u32, pub belt_hash: u64, pub belt_bytes: usize, pub link_ns: f64, pub days: u32 }

/// Where Ash's routine puts it at this tick (hand-written): the camp until 60, the stream until 120,
/// a berry bush until 180 (the oak every third day), the windmill until 240, then the camp; each
/// change up to 10 ticks early or late, hashed by day.
pub fn ash_target(seed: u64, t: u32) -> (i32, i32) {
    let l = layout(seed);
    let (d, tod) = (t / DAY, (t % DAY) as i32);
    let j = |k: u64| (hash(seed, d as u64, k, 0xa511) % 21) as i32 - 10;
    if tod < 60 + j(1) { l.camp } else if tod < 120 + j(2) { l.stream } else if tod < 180 + j(3) { if d % 3 == 2 { l.oak } else { l.bushes[0] } } else if tod < 240 + j(4) { l.windmill } else { l.camp }
}

/// `routine`: Ash's body follows `ash_target` (one cell a tick, overriding where its own mind
/// walks it); otherwise Ash lives as any villager.
pub fn run(seed: u64, people: usize, days: u32, maker: &str) -> Demo { run_with(seed, people, days, maker, true) }
pub fn run_with(seed: u64, people: usize, days: u32, maker: &str, routine: bool) -> Demo {
    let s = Setup::new(seed, people, days);
    let mut v = build(&s);
    let ord: Vec<usize> = (0..people).collect();
    let mut belt = Belt::with_maker(ToolConfig { on: true, day_len: DAY, ..ToolConfig::default() }, maker);
    let mut teller = Teller::default();
    let mut d = Demo { days, ..Demo::default() };
    let ash_node = v.folk[BRYN].pnode[ASH];
    let pattern = [ash_node];
    let ask_at: Vec<u32> = (0..days).flat_map(|day| (0..4).map(move |k| day * DAY + 20 + k as u32 * 55 + (hash(seed, day as u64, k, 0xa5a) % 40) as u32)).collect();
    for _ in 0..days * DAY {
        let t = v.w.tick;
        set_tiers(&mut v, &s);
        let night = v.w.script.iter().any(|(at, e)| *at == t && *e == Event::Night);
        // Questions are asked of Bryn's mind as it is at the start of the tick.
        if ask_at.contains(&t) && !v.w.people[BRYN].asleep {
            let p = &v.folk[BRYN];
            let m = &p.m;
            let mut q = Question { tick: t, from: (v.w.people[BRYN].x, v.w.people[BRYN].y), places: m.learnt_places(), ..Question::default() };
            let t0 = std::time::Instant::now();
            let (_, act, visits) = m.imagine(&[(ash_node, 1000)], &m.mods, 3);
            let mut ranked: Vec<(i32, usize)> = q.places.iter().map(|&(pi, _, _)| (act[m.place_nodes[pi] as usize], pi)).filter(|x| x.0 > 0).collect();
            ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            q.think = ranked.into_iter().map(|x| x.1).collect();
            q.think_ns = t0.elapsed().as_nanos() as u64;
            q.think_cost = visits;
            if belt.bound(&pattern).is_some() {
                let mut clone = m.clone();
                let mut sense = v.w.sense(BRYN, false);
                sense.cues.push((ash_node, 1000));
                clone.step(&sense);
                let ws = clone.last.ws.clone();
                q.fired = ws.contains(&ash_node);
                q.step_visits = clone.last.visits;
                let ctx = rec_of(&[(F::Who, ASH as i32)]);
                let places = m.place_nodes.clone();
                let t1 = std::time::Instant::now();
                let (cues, fuel, _) = belt.tick(&ws, t, &ctx, Reach::calm(Brain::Slow), &|v| places.get(v.max(0) as usize).copied());
                q.tool_ns = t1.elapsed().as_nanos() as u64;
                q.tool_fuel = fuel;
                if q.fired {
                    q.tool = Some(cues.iter().filter_map(|c| places.iter().position(|&n| n == c.0)).collect());
                    let mut s2 = v.w.sense(BRYN, false);
                    s2.tick += 1;
                    s2.cues = cues.clone();
                    clone.step(&s2);
                    q.step_visits += clone.last.visits;
                    q.came_to_mind = cues.first().map(|c| clone.last.ws.contains(&c.0));
                }
                // Reset the quiet time: questions here are hours apart anyway.
                belt.binds.iter_mut().for_each(|b| b.last = None);
            }
            if q.tool.is_none() { belt.thought(where_is(ASH as i32), &pattern, visits as i32, t); }
            d.qs.push(q);
        }
        let (ax, ay) = (v.w.people[ASH].x, v.w.people[ASH].y);
        v.step(1, &ord);
        if routine { let (tx, ty) = ash_target(seed, t); let a = &mut v.w.people[ASH]; a.x = ax + (tx - ax).signum(); a.y = ay + (ty - ay).signum(); a.asleep = false; }
        // Bryn's sightings of Ash.
        let m = &v.folk[BRYN].m;
        for e in v.w.sense(BRYN, false).seen.iter() {
            if person_of(e.id, people) != Some(ASH) { continue; }
            let pl = m.place_index_at(e.x, e.y).map_or(NONE, |p| p as i32);
            if teller.tell(BRYN, ASH as i32, pl, t) { d.sightings += 1; belt.tell(rec_of(&[(F::Kind, kind::SEEN), (F::Who, ASH as i32), (F::Place, pl), (F::X, e.x), (F::Y, e.y)]), t); }
        }
        d.ash.push((v.w.people[ASH].x, v.w.people[ASH].y));
        if night { if belt.sleep(t).is_some() { let m = belt.made.last().unwrap(); d.made.push((t / DAY, m.4.clone(), belt.tools[m.2].desc.to_string())); } }
    }
    d.belt_hash = belt.state_hash();
    d.belt_bytes = belt.bytes();
    // A link read, timed: every node's links in Bryn's store, a few times over.
    let st = &v.folk[BRYN].m.store;
    let mut out = vec![];
    let (mut n, t0) = (0u64, std::time::Instant::now());
    for _ in 0..20 { for u in 0..st.len() as u32 { out.clear(); st.edges(u, &mut out); n += out.len() as u64; } }
    d.link_ns = t0.elapsed().as_nanos() as f64 / n.max(1) as f64;
    d
}

/// Ticks to find Ash from a question, looking at `order` first, then everywhere else nearest first.
pub fn time_to_find(d: &Demo, q: &Question, order: &[usize]) -> Option<u32> {
    let mut todo: Vec<(i32, i32)> = order.iter().filter_map(|&pi| q.places.iter().find(|p| p.0 == pi).map(|p| (p.1, p.2))).collect();
    let mut rest: Vec<(i32, i32)> = q.places.iter().filter(|p| !order.contains(&p.0)).map(|p| (p.1, p.2)).collect();
    let (mut x, mut y) = q.from;
    // Nearest first, each from where the walk has got to.
    while !rest.is_empty() { let (k, _) = rest.iter().enumerate().min_by_key(|(_, p)| ((p.0 - x).abs().max((p.1 - y).abs()), p.0, p.1)).unwrap(); let p = rest.remove(k); todo.push(p); x = p.0; y = p.1; }
    let (mut x, mut y) = q.from;
    let mut k = 0;
    for dt in 0..GIVE_UP {
        let Some(&(ax, ay)) = d.ash.get((q.tick + dt) as usize) else { return None };
        if (ax - x).pow(2) + (ay - y).pow(2) <= SIGHT * SIGHT { return Some(dt); }
        if k < todo.len() { let (tx, ty) = todo[k]; if (x, y) == (tx, ty) { k += 1; } else { x += (tx - x).signum(); y += (ty - y).signum(); } }
    }
    None
}

/// The default maker, for the demo.
pub fn default_run(seed: u64, days: u32) -> Demo { run(seed, 12, days, kit::MAKER) }
