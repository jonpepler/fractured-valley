//! The people's valley: a 64 × 40 grid of 1 m cells with a camp, a few landmarks, a far stream
//! and a spring behind a ridge. Four people (Ash, Bryn, Cai, Dee) and paper notes. The world
//! knows nothing about minds: each person's mind reads what it senses of the world as it was at
//! the start of the tick (things, other people, notes on the ground, and the **sounds in the
//! air**, which are last tick's utterances), and returns an action, maybe an utterance, maybe a
//! note handed over or picked up. The world applies them all afterwards, in person order.
//!
//! Seeds move the landmarks and the spring by up to 3 cells, so different seeds are different
//! valleys (unlike the fox's, where the seed changed almost nothing).

use super::talk::{Content, Say};
use crate::mind::{Action, Body, Sense, Seen};
use crate::rng::{hash, StateHasher};
use std::hash::{Hash, Hasher};

pub const GRID_W: i32 = 64;
pub const GRID_H: i32 = 40;
pub const SIGHT: i32 = 10;
pub const PLACE_RADIUS: i32 = 6;
/// People within this many cells can talk face to face.
pub const TALK: i32 = 8;
pub const PEOPLE: usize = 4;
pub const NAMES: [&str; PEOPLE] = ["Ash", "Bryn", "Cai", "Dee"];

/// What a person can tell apart by looking (the index is the species' kind index). Each person
/// has their own look, so people are recognised by look, never by identity.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Kind { Ash, Bryn, Cai, Dee, Water, Rock, Windmill, Oak, Hut, Note, DryFresh, DryOld }
impl Kind {
    pub const ALL: [Kind; 12] = [Kind::Ash, Kind::Bryn, Kind::Cai, Kind::Dee, Kind::Water, Kind::Rock, Kind::Windmill, Kind::Oak, Kind::Hut, Kind::Note, Kind::DryFresh, Kind::DryOld];
    pub fn k(self) -> u8 { self as u8 }
    pub fn person(i: usize) -> Kind { Kind::ALL[i] }
    pub fn name(self) -> &'static str {
        match self { Kind::Ash => "Ash", Kind::Bryn => "Bryn", Kind::Cai => "Cai", Kind::Dee => "Dee", Kind::Water => "water", Kind::Rock => "rock", Kind::Windmill => "windmill",
            Kind::Oak => "oak", Kind::Hut => "hut", Kind::Note => "note", Kind::DryFresh => "freshly dry hollow", Kind::DryOld => "long-dry hollow" }
    }
    pub fn landmark(self) -> bool { matches!(self, Kind::Water | Kind::Rock | Kind::Windmill | Kind::Oak | Kind::Hut | Kind::DryFresh | Kind::DryOld) }
}

/// A fixed thing: a landmark, a water source, a dry hollow.
#[derive(Clone, Debug, Hash)]
pub struct Thing { pub id: u32, pub kind: Kind, pub name: String, pub x: i32, pub y: i32, pub alive: bool }

/// A paper note: on the ground or carried, with what was written on it and by whom.
#[derive(Clone, Debug, Hash)]
pub struct Note { pub id: u32, pub x: i32, pub y: i32, pub holder: Option<usize>, pub content: Content, pub writer: usize }

#[derive(Clone, Debug, Hash)]
pub struct Person { pub x: i32, pub y: i32, pub px: i32, pub py: i32, pub thirst: i32, pub asleep: bool, pub drinks: u32, pub first_drink: Option<(u32, u32)>,
    /// Staging: gone away (out of the valley, out of sight and earshot; its mind isn't stepped).
    pub away: bool }

/// An utterance in the air: who said it, where, how loud, who they faced, and what.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Utterance { pub tick: u32, pub speaker: usize, pub x: i32, pub y: i32, pub vol: i32, pub to: Option<usize>, pub say: Say }

/// Things a scenario can make happen at a tick (never a choice of a person's).
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum Event {
    /// Dusk: from now until night, people's thoughts turn to rest (they walk home if it pulls).
    Dusk,
    /// Night: everyone sleeps **where they are**.
    Night,
    SetThirst { who: usize, v: i32 },
    /// The spring dries: its water goes, a freshly dry hollow is left.
    DrySpring,
    /// A freshly dry hollow cracks and looks long dry.
    AgeHollow,
    /// Place someone (the start of a scenario only).
    Put { who: usize, x: i32, y: i32 },
    /// `who` is led by `by` (walks beside them) until `until` (a demonstration).
    Lead { who: usize, by: usize, until: u32 },
    /// `who` drops the notes they carry without noticing.
    LoseNotes { who: usize },
    /// Put a lost note on the ground somewhere (a scenario that starts with a lost note).
    MoveNote { id: u32, x: i32, y: i32 },
    /// `who` leaves the valley for good (staging).
    Away { who: usize },
}

/// What the world receives from one person in a tick.
#[derive(Clone, Debug, Default, Hash)]
pub struct Deed { pub action: Option<Action>, pub say: Option<(Say, Option<usize>, i32)>, pub write: Option<(Content, usize)>, pub pick_up: Option<u32> }

#[derive(Clone, Debug)]
pub struct World {
    pub seed: u64,
    pub tick: u32,
    pub things: Vec<Thing>,
    pub notes: Vec<Note>,
    pub people: Vec<Person>,
    /// Last tick's utterances (what can be heard this tick).
    pub air: Vec<Utterance>,
    pub script: Vec<(u32, Event)>,
    pub log: Vec<(u32, String)>,
    pub next_id: u32,
    pub night_now: bool,
    pub dusk: bool,
    pub leads: Vec<(usize, usize, u32)>,
    /// Every utterance ever made (for measures and the viewer).
    pub said: Vec<Utterance>,
}

pub const CAMP: u32 = 9;
pub const STREAM: u32 = 10;
pub const ROCK: u32 = 11;
pub const WINDMILL: u32 = 12;
pub const OAK: u32 = 13;
pub const SPRING: u32 = 14;
/// Ids of people as things that can be seen: 1..=4.
pub fn person_id(i: usize) -> u32 { i as u32 + 1 }

pub use crate::mind::{d2, dist};

pub fn step_toward(x: i32, y: i32, tx: i32, ty: i32) -> (i32, i32) {
    let (dx, dy) = (tx - x, ty - y);
    let (sx, sy) = (dx.signum(), dy.signum());
    if dx.abs() > 2 * dy.abs() { (x + sx, y) } else if dy.abs() > 2 * dx.abs() { (x, y + sy) } else { (x + sx, y + sy) }
}

/// Loudness at `d` cells: `vol · 100 / (100 + d²)` (as in `spikes/brains-talk`).
pub fn loudness(vol: i32, d2: i32) -> i32 { (vol as i64 * 100 / (100 + d2 as i64)) as i32 }
/// Below this nothing is heard; at or above `CLEAR` everything is. In between, a hashed roll
/// decides whether it is caught or missed (never misheard: no noise is added to what is said).
pub const HEAR_MIN: i32 = 60;
pub const CLEAR: i32 = 300;
pub const SPEAK: i32 = 500;

impl World {
    /// The valley for a seed: the camp is fixed; every other landmark and the spring are moved by
    /// up to 3 cells by the seed.
    pub fn valley(seed: u64) -> World {
        let j = |k: u64| (hash(seed, k, 0, 0x7a11) % 7) as i32 - 3;
        let t = |id, kind, name: &str, x: i32, y: i32| Thing { id, kind, name: name.into(), x, y, alive: true };
        let things = vec![
            t(CAMP, Kind::Hut, "the hut", 10, 22),
            t(STREAM, Kind::Water, "the stream", 56 + j(1), 36 + j(2).min(2)),
            t(ROCK, Kind::Rock, "the big rock", 33 + j(3), 11 + j(4)),
            t(WINDMILL, Kind::Windmill, "the windmill", 47 + j(5), 18 + j(6)),
            t(OAK, Kind::Oak, "the old oak", 24 + j(7), 5 + j(8)),
            t(SPRING, Kind::Water, "the spring", 40 + j(9), 6 + j(10)),
        ];
        let people = (0..PEOPLE).map(|_| Person { x: 10, y: 23, px: 10, py: 23, thirst: 200, asleep: false, drinks: 0, first_drink: None, away: false }).collect();
        World { seed, tick: 0, things, notes: vec![], people, air: vec![], script: vec![], log: vec![], next_id: 100, night_now: false, dusk: false, leads: vec![], said: vec![] }
    }
    pub fn thing(&self, id: u32) -> &Thing { self.things.iter().find(|t| t.id == id).expect("thing") }
    pub fn spring(&self) -> (i32, i32) { let s = self.thing(SPRING); (s.x, s.y) }

    /// What person `i` senses: things, other people and notes on the ground in sight (a circle,
    /// no line of sight), its body. Hearing is read separately (`heard`).
    pub fn sense(&self, i: usize, awake: bool) -> Sense {
        let p = &self.people[i];
        let mut seen = vec![];
        if !p.asleep || awake {
            let near = |x: i32, y: i32| d2(x, y, p.x, p.y) <= SIGHT * SIGHT;
            for t in self.things.iter().filter(|t| t.alive && near(t.x, t.y)) {
                seen.push(Seen { id: t.id, kind: t.kind.k(), x: t.x, y: t.y, approaching: false, available: t.kind == Kind::Water });
            }
            for (j, q) in self.people.iter().enumerate() {
                if j == i || q.away || !near(q.x, q.y) { continue; }
                seen.push(Seen { id: person_id(j), kind: Kind::person(j).k(), x: q.x, y: q.y, approaching: d2(q.x, q.y, p.x, p.y) < d2(q.px, q.py, p.x, p.y), available: false });
            }
            for n in self.notes.iter().filter(|n| n.holder.is_none() && near(n.x, n.y)) {
                seen.push(Seen { id: n.id, kind: Kind::Note.k(), x: n.x, y: n.y, approaching: false, available: false });
            }
        }
        Sense { tick: self.tick, seed: self.seed, body: Body { x: p.x, y: p.y, hunger: p.thirst, pain: 0, asleep: p.asleep && !awake }, seen, cues: vec![] }
    }

    /// The utterances in the air that person `i` catches this tick: (utterance, loudness,
    /// addressed to me). A hashed roll decides the in-between loudnesses.
    pub fn heard(&self, i: usize) -> Vec<(Utterance, i32, bool)> {
        let p = &self.people[i];
        if p.asleep { return vec![]; }
        let mut out = vec![];
        for u in &self.air {
            if u.speaker == i { continue; }
            let loud = loudness(u.vol, d2(u.x, u.y, p.x, p.y));
            if loud < HEAR_MIN { continue; }
            if loud < CLEAR {
                let miss = (CLEAR - loud) * 1000 / (CLEAR - HEAR_MIN);
                if ((hash(self.seed, self.tick as u64, i as u64, 0x4ea2 ^ u.speaker as u64) % 1000) as i32) < miss { continue; }
            }
            out.push((u.clone(), loud, u.to == Some(i)));
        }
        out
    }

    /// Apply the script. Call before minds read the world.
    pub fn pre_step(&mut self) {
        self.night_now = false;
        for p in self.people.iter_mut() { p.px = p.x; p.py = p.y; }
        let evs: Vec<Event> = self.script.iter().filter(|(t, _)| *t == self.tick).map(|(_, e)| e.clone()).collect();
        for ev in evs {
            match ev {
                Event::Dusk => { self.dusk = true; }
                Event::Night => { self.dusk = false; self.night_now = true; for p in self.people.iter_mut() { p.asleep = true; } self.log.push((self.tick, "night: everyone sleeps where they are".into())); }
                Event::SetThirst { who, v } => { self.people[who].thirst = v; }
                Event::DrySpring => {
                    let (x, y) = self.spring();
                    if let Some(s) = self.things.iter_mut().find(|t| t.id == SPRING) { s.alive = false; }
                    let id = self.next_id; self.next_id += 1;
                    self.things.push(Thing { id, kind: Kind::DryFresh, name: "a dry hollow".into(), x, y, alive: true });
                    self.log.push((self.tick, "the spring dries up".into()));
                }
                Event::AgeHollow => { for t in self.things.iter_mut().filter(|t| t.kind == Kind::DryFresh) { t.kind = Kind::DryOld; } }
                Event::Put { who, x, y } => { let p = &mut self.people[who]; p.x = x; p.y = y; p.px = x; p.py = y; }
                Event::Lead { who, by, until } => { self.leads.push((who, by, until)); }
                Event::LoseNotes { who } => {
                    let (x, y) = (self.people[who].x, self.people[who].y);
                    for n in self.notes.iter_mut().filter(|n| n.holder == Some(who)) { n.holder = None; n.x = x; n.y = y; }
                    self.log.push((self.tick, format!("{} drops a note without noticing", NAMES[who])));
                }
                Event::Away { who } => { let p = &mut self.people[who]; p.away = true; p.x = 63; p.y = 39; p.px = 63; p.py = 39; }
                Event::MoveNote { id, x, y } => { if let Some(n) = self.notes.iter_mut().find(|n| n.id == id) { n.holder = None; n.x = x; n.y = y; } }
            }
        }
    }

    /// Apply every person's deed (in person order), their bodies' needs, and the sounds they
    /// made (which are heard next tick). Then advance the tick.
    pub fn post_step(&mut self, deeds: &[Deed]) {
        let tick = self.tick;
        let mut air = vec![];
        for (i, d) in deeds.iter().enumerate() {
            if let Some(a) = d.action {
                let p = &mut self.people[i];
                match a {
                    Action::WalkTo { x, y } => { let (nx, ny) = step_toward(p.x, p.y, x, y); p.x = nx.clamp(0, GRID_W - 1); p.y = ny.clamp(0, GRID_H - 1); }
                    Action::Flee { from_x, from_y, .. } => { let (nx, ny) = step_toward(p.x, p.y, 2 * p.x - from_x, 2 * p.y - from_y); p.x = nx.clamp(0, GRID_W - 1); p.y = ny.clamp(0, GRID_H - 1); }
                    Action::Eat { entity } => {
                        let (px, py) = (p.x, p.y);
                        if self.things.iter().any(|t| t.id == entity && t.alive && t.kind == Kind::Water && (t.x - px).abs().max((t.y - py).abs()) <= 1) {
                            let p = &mut self.people[i];
                            p.thirst = (p.thirst - 500).max(0); p.drinks += 1;
                            if p.first_drink.is_none() { p.first_drink = Some((tick, entity)); }
                            self.log.push((tick, format!("{} drinks from {}", NAMES[i], self.thing(entity).name)));
                        }
                    }
                    _ => {}
                }
            }
            if let Some((say, to, vol)) = &d.say {
                let p = &self.people[i];
                let u = Utterance { tick, speaker: i, x: p.x, y: p.y, vol: *vol, to: *to, say: say.clone() };
                self.log.push((tick, format!("{} says to {}: {}", NAMES[i], to.map(|t| NAMES[t]).unwrap_or("anyone"), u.say.render())));
                air.push(u);
            }
            if let Some((content, to)) = &d.write {
                let id = self.next_id; self.next_id += 1;
                let (x, y) = (self.people[*to].x, self.people[*to].y);
                self.notes.push(Note { id, x, y, holder: Some(*to), content: content.clone(), writer: i });
                self.log.push((tick, format!("{} writes a note and hands it to {}", NAMES[i], NAMES[*to])));
            }
            if let Some(id) = d.pick_up {
                let (x, y) = (self.people[i].x, self.people[i].y);
                if let Some(n) = self.notes.iter_mut().find(|n| n.id == id && n.holder.is_none() && (n.x - x).abs().max((n.y - y).abs()) <= 1) {
                    n.holder = Some(i);
                    self.log.push((tick, format!("{} picks up a note", NAMES[i])));
                }
            }
        }
        // Being led: walk beside the leader (a demonstration), whatever the mind chose.
        self.leads.retain(|l| l.2 > tick);
        for &(who, by, _) in &self.leads.clone() {
            let (lx, ly) = (self.people[by].x, self.people[by].y);
            let p = &mut self.people[who];
            p.x = (lx - 1).max(0); p.y = ly;
        }
        for (i, p) in self.people.iter_mut().enumerate() {
            if p.asleep { p.asleep = false; }
            p.thirst = (p.thirst + 2).min(1000);
            for n in self.notes.iter_mut().filter(|n| n.holder == Some(i)) { n.x = p.x; n.y = p.y; }
        }
        self.said.extend(air.iter().cloned());
        self.air = air;
        self.tick += 1;
    }

    pub fn hash_into<H: Hasher>(&self, h: &mut H) {
        self.tick.hash(h); self.things.hash(h); self.notes.hash(h); self.people.hash(h); self.air.hash(h); self.leads.hash(h);
    }
    pub fn state_hash(&self) -> u64 { let mut h = StateHasher::new(self.seed); self.hash_into(&mut h); h.finish() }
}
