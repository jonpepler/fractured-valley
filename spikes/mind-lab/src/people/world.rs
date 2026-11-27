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
/// Stage 5: people are a vector of any size. The first four keep their names, ids and looks, so
/// every earlier scenario is unchanged; the rest are named from this list (debug labels and logs
/// only: names are never hashed and never read by a mind).
pub const MORE_NAMES: [&str; 60] = ["Eli", "Fen", "Gil", "Hal", "Ivo", "Jun", "Kit", "Lev", "Mo", "Nell", "Oda", "Pip", "Quin", "Rue", "Sol", "Tam", "Uma", "Vi", "Wren", "Xan",
    "Yew", "Zed", "Abe", "Bea", "Cy", "Dot", "Edda", "Fay", "Gus", "Hob", "Ida", "Jem", "Kay", "Lux", "Mab", "Ned", "Ora", "Pell", "Rook", "Sid",
    "Tess", "Ulf", "Val", "Wim", "Yara", "Zia", "Arlo", "Bram", "Cleo", "Dov", "Esme", "Finn", "Gwen", "Hugo", "Isla", "Jory", "Kes", "Lark", "Mira", "Noor"];
pub fn name(j: usize) -> &'static str { if j < PEOPLE { NAMES[j] } else { MORE_NAMES.get(j - PEOPLE).copied().unwrap_or("someone") } }
/// Stage 5: what person `j` looks like (a kind index). The first four are `Kind::Ash..Dee`; later
/// people come after the kinds every species tells apart today (`Kind::ALL`) and the village's
/// own (`Kind::Berry`, `Kind::Fox`), so no earlier species' kind table changes.
pub fn person_kind(j: usize) -> u8 { if j < PEOPLE { j as u8 } else { (Kind::Fox as usize + 1 + j - PEOPLE) as u8 } }
/// The number of kinds a species must tell apart in a valley of `n` people (the village's kinds
/// only when `village`).
pub fn kind_count(n: usize, village: bool) -> usize { if village || n > PEOPLE { Kind::Fox as usize + 1 + n.saturating_sub(PEOPLE) } else { Kind::ALL.len() } }

/// What a person can tell apart by looking (the index is the species' kind index). Each person
/// has their own look, so people are recognised by look, never by identity.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Kind { Ash, Bryn, Cai, Dee, Water, Rock, Windmill, Oak, Hut, Note, DryFresh, DryOld, Boar, Deer,
    /// Stage 5 (the village only; not in `ALL`, so earlier species' kind tables are unchanged):
    /// a berry bush, and a fox.
    Berry, Fox }
impl Kind {
    pub const ALL: [Kind; 14] = [Kind::Ash, Kind::Bryn, Kind::Cai, Kind::Dee, Kind::Water, Kind::Rock, Kind::Windmill, Kind::Oak, Kind::Hut, Kind::Note, Kind::DryFresh, Kind::DryOld, Kind::Boar, Kind::Deer];
    pub fn k(self) -> u8 { self as u8 }
    pub fn person(i: usize) -> Kind { Kind::ALL[i] }
    pub fn name(self) -> &'static str {
        match self { Kind::Ash => "Ash", Kind::Bryn => "Bryn", Kind::Cai => "Cai", Kind::Dee => "Dee", Kind::Water => "water", Kind::Rock => "rock", Kind::Windmill => "windmill",
            Kind::Oak => "oak", Kind::Hut => "hut", Kind::Note => "note", Kind::DryFresh => "freshly dry hollow", Kind::DryOld => "long-dry hollow", Kind::Boar => "boar", Kind::Deer => "deer", Kind::Berry => "berry bush", Kind::Fox => "fox" }
    }
    pub fn landmark(self) -> bool { matches!(self, Kind::Water | Kind::Rock | Kind::Windmill | Kind::Oak | Kind::Hut | Kind::DryFresh | Kind::DryOld | Kind::Berry) }
    /// Stage 4: things a person can hide behind (a person keeping still right by one is hidden).
    pub fn cover(self) -> bool { matches!(self, Kind::Rock | Kind::Windmill | Kind::Oak | Kind::Hut) }
}

/// A fixed thing: a landmark, a water source, a dry hollow.
#[derive(Clone, Debug, Hash)]
pub struct Thing { pub id: u32, pub kind: Kind, pub name: String, pub x: i32, pub y: i32, pub alive: bool }

/// A paper note: on the ground or carried, with what was written on it and by whom.
#[derive(Clone, Debug, Hash)]
pub struct Note { pub id: u32, pub x: i32, pub y: i32, pub holder: Option<usize>, pub content: Content, pub writer: usize }

#[derive(Clone, Debug, Hash)]
pub struct Person { pub x: i32, pub y: i32, pub px: i32, pub py: i32, pub thirst: i32, pub asleep: bool, pub drinks: u32,
    /// Pain from a wound (stage 3), fading by a tenth a tick; times gored (at `DOWN_AFTER`, down).
    pub pain: i32, pub gored: u32, pub first_drink: Option<(u32, u32)>,
    /// Staging: gone away (out of the valley, out of sight and earshot; its mind isn't stepped).
    pub away: bool }

/// Stage 3: a wild boar, with a mind of its own (`crate::danger::boar`). Its id as a thing seen is
/// `BOAR_ID + index`.
#[derive(Clone, Debug, Hash)]
pub struct Beast { pub x: i32, pub y: i32, pub px: i32, pub py: i32, pub pain: i32, pub asleep: bool,
    /// Ticks it has been struck, been braced against, and gored someone (for measures).
    pub struck: u32, pub braced: u32, pub gored: u32,
    /// Broken off (badly hurt), running from where it was hurt.
    pub routed: Option<(i32, i32)> }
pub const BOAR_ID: u32 = 50;
/// A person's blow at a boar within 2 cells; a boar's goring of a person; and what bracing does
/// when a boar charges into someone holding their ground (the boar is hurt, the holder less so).
pub const STRIKE: i32 = 250;
pub const GORE: i32 = 400;
pub const BRACE: i32 = 350;
pub const BRACED_GORE: i32 = 150;
/// A cry for help carries this loud (a talk is `SPEAK`).
pub const SHOUT: i32 = 2500;
/// Bodies have limits (world rules, not minds): gored this many times, a person is down (it can
/// no longer move or strike); struck or braced against this many times, a boar is badly hurt and
/// breaks off, running for its wallow's side of the valley.
pub const DOWN_AFTER: u32 = 3;
pub const ROUT_AFTER: u32 = 4;

/// Stage 4: a deer, with a mind of its own (`crate::hunt::deer`). Its id as a thing seen is
/// `DEER_ID + index`. It grows hungry, grazes where it stands on one of the valley's meadows, and
/// can be caught: a person's strike within 2 cells lands with probability `HIT` per mille.
#[derive(Clone, Debug, Hash)]
pub struct Deer { pub x: i32, pub y: i32, pub px: i32, pub py: i32, pub hunger: i32, pub asleep: bool,
    /// Caught: (tick, by whom).
    pub caught: Option<(u32, usize)>,
    /// Ticks it ran (moved 2 cells), and strikes that missed.
    pub ran: u32, pub missed: u32,
    /// Got away (unused: the valley is walled; kept for a version with exits).
    pub escaped: Option<u32> }
pub const DEER_ID: u32 = 60;
/// A strike at a deer within 2 cells lands this often (per mille, a hashed roll); a landed strike
/// catches it. (A world rule, like the boar's wounds.)
pub const HIT: i32 = 700;
/// Grazing: a deer keeping still within this many cells of a meadow eats this much a tick.
pub const GRAZE_R: i32 = 2;
pub const GRAZE: i32 = 12;
/// A person who has not moved for a tick, right by (within `COVER_R` cells of) a thing that can
/// hide it, is hidden from anyone further off than `SPOT_R` cells (when `World::cover` is on).
pub const COVER_R: i32 = 2;
pub const SPOT_R: i32 = 3;

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
    /// Stage 3: place a boar (the start of a scenario; staging).
    PutBoar { b: usize, x: i32, y: i32 },
    /// Stage 4: someone away comes back, at a place (staging).
    Back { who: usize, x: i32, y: i32 },
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
    pub dusk_at: u32,
    pub leads: Vec<(usize, usize, u32)>,
    /// Every utterance ever made (for measures and the viewer).
    pub said: Vec<Utterance>,
    /// Stage 3: boars.
    pub boars: Vec<Beast>,
    /// Stage 4 (the hunt): deer; the meadows they graze (world truth); hiding behind cover on or
    /// off; ticks each person has kept still; blows struck between people (tick, by, at).
    /// None of these is hashed while unused, so earlier stages' world hashes are unchanged.
    pub deer: Vec<Deer>,
    pub meadows: Vec<(i32, i32)>,
    pub cover: bool,
    pub still: Vec<u32>,
    pub blows: Vec<(u32, usize, usize)>,
    /// Stage 5: the village's state (hunger and food, berry bushes, foxes, the voice model), hashed
    /// only when present, so no earlier stage's world hash moves.
    pub village: Option<Box<VillageState>>,
}

/// Stage 5: a berry bush's fruit (world truth): servings left, at most `max`, one more every
/// `regrow` ticks.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Bush { pub id: u32, pub servings: i32, pub max: i32, pub regrow: u32 }

/// Stage 5: a fox's body (its mind is `crate::village::fox`). Its id as a thing seen is `FOX_ID + k`.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct FoxBody { pub x: i32, pub y: i32, pub px: i32, pub py: i32, pub hunger: i32, pub asleep: bool, pub meals: u32 }
pub const FOX_ID: u32 = 70;

/// Stage 5: what the village adds to the valley.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct VillageState {
    /// Each person's hunger (0..1000, +`HUNGER_RATE` a tick; a serving of berries takes `MEAL` off).
    pub hunger: Vec<i32>,
    pub meals: Vec<u32>,
    pub bushes: Vec<Bush>,
    pub foxes: Vec<FoxBody>,
    /// The voice model (`voice_level`): loudness falls with distance as sound does, a solid thing
    /// between speaker and hearer takes `BARRIER` off, and other voices mask quieter ones.
    pub voice: bool,
    /// Wounds heal: one goring a night (a person three times gored is otherwise down for good).
    pub heal: bool,
    /// The deer comes back to its meadow the morning after it is caught (world rule).
    pub deer_home: Vec<(i32, i32)>,
    /// Meals from a caught deer: (tick, who ate).
    pub feasts: Vec<(u32, u32)>,
    /// Each boar's hunger (in the village a boar forages at the bushes) and meals.
    pub boar_hunger: Vec<i32>,
    pub boar_meals: Vec<u32>,
}
pub const HUNGER_RATE: i32 = 1;
pub const MEAL: i32 = 400;
/// A caught deer feeds everyone within this many cells of it (a shared meal: a world rule).
pub const FEAST_R: i32 = 4;
pub const FEAST: i32 = 600;

// ------------------------------------------------------------------------- stage 5: the voice
/// Speech levels at 1 m, in tenths of a decibel (ANSI S3.5-1997 speech levels: normal about 60 dB,
/// raised 66, shouted 77 to 82; a whisper about 30 to 35; a muted voice about 50).
pub fn voice_db(vol: i32) -> i32 { match vol { v if v <= LOW => 500, v if v < 0 => 350, v if v >= SHOUT => 800, _ => 600 } }
/// The village's background noise by day (tenths of a dB): a lived-in rural place, 40 dB(A).
pub const AMBIENT: i32 = 400;
/// A solid landmark near the line between speaker and hearer takes this off (a barrier's insertion
/// loss: 5 to 15 dB for a low wall or a building; one barrier counted).
pub const BARRIER: i32 = 100;
/// Catching speech by signal-to-noise ratio (tenths of a dB): never below `SNR_MIN`, always at or
/// above `SNR_CLEAR`, a hashed roll in between (sentence intelligibility rises from near 0 to near
/// 100% over about -6 to +6 dB SNR for normal hearing).
pub const SNR_MIN: i32 = -60;
pub const SNR_CLEAR: i32 = 60;

/// `10·log10(x)` in tenths of a decibel, for x ≥ 1, in integers: `log2` to 8 fractional bits by
/// repeated squaring, times 10·log10(2) = 3.0103.
pub fn db10(x: u32) -> i32 {
    let x = x.max(1);
    let ip = 31 - x.leading_zeros();
    // x / 2^ip in [1, 2), held in Q30.
    let mut m: u64 = if ip <= 30 { (x as u64) << (30 - ip) } else { (x as u64) >> (ip - 30) };
    let mut frac = 0u32;
    for _ in 0..8 {
        m = (m * m) >> 30;
        frac <<= 1;
        if m >= 2 << 30 { m >>= 1; frac |= 1; }
    }
    let log2_q8 = ((ip as i64) << 8) + frac as i64;
    (log2_q8 * 30103 / 25600) as i32
}

/// The level of utterance `u` heard at `(x, y)` (tenths of a dB): its voice at 1 m, less spherical
/// spreading (6 dB per doubling of distance), less one barrier if a solid landmark stands near the
/// line between (within a cell of it, and not at either end).
pub fn voice_level(w: &World, u: &Utterance, x: i32, y: i32) -> i32 {
    let d2v = d2(u.x, u.y, x, y).max(1) as u32;
    let mut l = voice_db(u.vol) - db10(d2v);
    if w.things.iter().any(|t| t.alive && t.kind.cover() && blocks(u.x, u.y, x, y, t.x, t.y)) { l -= BARRIER; }
    l
}
/// Does a solid thing at `(tx, ty)` stand within a cell of the segment from a to b, away from both ends?
pub fn blocks(ax: i32, ay: i32, bx: i32, by: i32, tx: i32, ty: i32) -> bool {
    if (tx - ax).abs().max((ty - ay).abs()) <= 1 || (tx - bx).abs().max((ty - by).abs()) <= 1 { return false; }
    let (vx, vy, wx, wy) = ((bx - ax) as i64, (by - ay) as i64, (tx - ax) as i64, (ty - ay) as i64);
    let len2 = vx * vx + vy * vy;
    if len2 == 0 { return false; }
    let t = wx * vx + wy * vy;
    if t <= 0 || t >= len2 { return false; }
    // Squared distance from the line, times len2: cross² ≤ len2 (within one cell).
    let cross = wx * vy - wy * vx;
    cross * cross <= len2
}

pub const CAMP: u32 = 9;
pub const STREAM: u32 = 10;
pub const ROCK: u32 = 11;
pub const WINDMILL: u32 = 12;
pub const OAK: u32 = 13;
pub const SPRING: u32 = 14;
/// Ids of people as things that can be seen: 1..=4 for the first four (as before), then
/// `PERSON_ID0 + i` (stage 5: clear of the landmarks', boars', deer's and notes' ids).
pub fn person_id(i: usize) -> u32 { if i < PEOPLE { i as u32 + 1 } else { PERSON_ID0 + i as u32 } }
pub const PERSON_ID0: u32 = 1000;
/// The person an id belongs to, if any, in a valley of `n` people.
pub fn person_of(id: u32, n: usize) -> Option<usize> {
    let j = if (1..=PEOPLE as u32).contains(&id) { id as usize - 1 } else if id >= PERSON_ID0 + PEOPLE as u32 { (id - PERSON_ID0) as usize } else { return None };
    (j < n).then_some(j)
}

pub use crate::mind::{d2, dist};

pub fn step_toward(x: i32, y: i32, tx: i32, ty: i32) -> (i32, i32) {
    let (dx, dy) = (tx - x, ty - y);
    let (sx, sy) = (dx.signum(), dy.signum());
    if dx.abs() > 2 * dy.abs() { (x + sx, y) } else if dy.abs() > 2 * dx.abs() { (x, y + sy) } else { (x + sx, y + sy) }
}

/// The valley is walled by ridges with rounded corners: a fleeing deer never runs into a cell
/// this close to two walls at once (so it cannot be pinned in a corner, only between hunters).
pub fn corner(x: i32, y: i32) -> bool { x.min(GRID_W - 1 - x) + y.min(GRID_H - 1 - y) < 4 }
/// Stage 4: one step of flight on the grid. A deer runs for open ground: of the 8 directions (and
/// straight away), it takes the one whose run of `FLEE_LOOK` cells (stopping at the walls) ends
/// furthest from what it flees; ties go to straight away, then a fixed order.
pub fn flee_step(x: i32, y: i32, fx: i32, fy: i32) -> (i32, i32) {
    let (ax, ay) = step_toward(x, y, 2 * x - fx, 2 * y - fy);
    let ok = |nx: i32, ny: i32| nx >= 0 && ny >= 0 && nx < GRID_W && ny < GRID_H && !corner(nx, ny);
    let mut best = (d2(x, y, fx, fy), 0, x, y);
    for (k, (dx, dy)) in [(ax - x, ay - y), (1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)].into_iter().enumerate() {
        if (dx, dy) == (0, 0) || !ok(x + dx, y + dy) { continue; }
        let (mut ex, mut ey) = (x, y);
        for _ in 0..FLEE_LOOK { if !ok(ex + dx, ey + dy) { break; } ex += dx; ey += dy; }
        let c = (d2(ex, ey, fx, fy), if k == 0 { 1 } else { 0 }, x + dx, y + dy);
        if (c.0, c.1) > (best.0, best.1) { best = c; }
    }
    (best.2, best.3)
}
pub const FLEE_LOOK: i32 = 6;

/// Loudness at `d` cells: `vol · 100 / (100 + d²)` (as in `spikes/brains-talk`).
pub fn loudness(vol: i32, d2: i32) -> i32 {
    // Stage 4: a negative volume is a **whisper** of that strength: it falls off much faster
    // (clear at 1 cell, sometimes caught at 2 to 5, never beyond).
    if vol < 0 { return (-vol as i64 * 4 / (4 + d2 as i64)) as i32; }
    (vol as i64 * 100 / (100 + d2 as i64)) as i32
}
/// Stage 4: a whisper's strength (as a negative volume): clear within a cell, never past 5.
pub const WHISPER: i32 = -500;
/// Stage 4: a low voice (a plot): clear within about 6 cells, sometimes caught to 14.
pub const LOW: i32 = -3000;
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
        let people = (0..PEOPLE).map(|_| Person { x: 10, y: 23, px: 10, py: 23, thirst: 200, asleep: false, drinks: 0, pain: 0, gored: 0, first_drink: None, away: false }).collect();
        World { seed, tick: 0, things, notes: vec![], people, air: vec![], script: vec![], log: vec![], next_id: 100, night_now: false, dusk: false, dusk_at: 0, leads: vec![], said: vec![], boars: vec![], deer: vec![], meadows: vec![], cover: false, still: vec![], blows: vec![], village: None }
    }
    /// Evening sleepiness (stage 3): from dusk it rises from 400 by 10 a tick, to 900.
    pub fn drowsy(&self) -> i32 { if self.dusk { (400 + 10 * (self.tick - self.dusk_at) as i32).min(900) } else { 0 } }
    pub fn thing(&self, id: u32) -> &Thing { self.things.iter().find(|t| t.id == id).expect("thing") }
    pub fn spring(&self) -> (i32, i32) { let s = self.thing(SPRING); (s.x, s.y) }

    /// Stage 4: is person `j` hidden from a viewer at `(vx, vy)`? Only with `cover` on: it has kept
    /// still a tick or more right by something that can hide it, and the viewer is further off
    /// than `SPOT_R`.
    pub fn hidden(&self, j: usize, vx: i32, vy: i32) -> bool {
        if !self.cover || self.still.get(j).copied().unwrap_or(0) == 0 { return false; }
        let p = &self.people[j];
        if d2(p.x, p.y, vx, vy) <= SPOT_R * SPOT_R { return false; }
        self.things.iter().any(|t| t.alive && t.kind.cover() && (t.x - p.x).abs().max((t.y - p.y).abs()) <= COVER_R)
    }

    /// What person `i` senses: things, other people and notes on the ground in sight (a circle,
    /// no line of sight), its body. Hearing is read separately (`heard`).
    pub fn sense(&self, i: usize, awake: bool) -> Sense {
        let p = &self.people[i];
        let mut seen = vec![];
        if !p.asleep || awake {
            let near = |x: i32, y: i32| d2(x, y, p.x, p.y) <= SIGHT * SIGHT;
            for t in self.things.iter().filter(|t| t.alive && near(t.x, t.y)) {
                let available = match t.kind { Kind::Water => true, Kind::Berry => self.servings(t.id) > 0, _ => false };
                seen.push(Seen { id: t.id, kind: t.kind.k(), x: t.x, y: t.y, approaching: false, available });
            }
            for (j, q) in self.people.iter().enumerate() {
                if j == i || q.away || !near(q.x, q.y) || self.hidden(j, p.x, p.y) { continue; }
                seen.push(Seen { id: person_id(j), kind: person_kind(j), x: q.x, y: q.y, approaching: d2(q.x, q.y, p.x, p.y) < d2(q.px, q.py, p.x, p.y), available: false });
            }
            for (k, q) in self.deer.iter().enumerate().filter(|(_, q)| q.caught.is_none() && q.escaped.is_none() && near(q.x, q.y)) {
                seen.push(Seen { id: DEER_ID + k as u32, kind: Kind::Deer.k(), x: q.x, y: q.y, approaching: d2(q.x, q.y, p.x, p.y) < d2(q.px, q.py, p.x, p.y), available: false });
            }
            for (b, q) in self.boars.iter().enumerate().filter(|(_, q)| near(q.x, q.y)) {
                seen.push(Seen { id: BOAR_ID + b as u32, kind: Kind::Boar.k(), x: q.x, y: q.y, approaching: d2(q.x, q.y, p.x, p.y) < d2(q.px, q.py, p.x, p.y), available: false });
            }
            for n in self.notes.iter().filter(|n| n.holder.is_none() && near(n.x, n.y)) {
                seen.push(Seen { id: n.id, kind: Kind::Note.k(), x: n.x, y: n.y, approaching: false, available: false });
            }
            if let Some(v) = &self.village {
                for (k, q) in v.foxes.iter().enumerate().filter(|(_, q)| near(q.x, q.y)) {
                    seen.push(Seen { id: FOX_ID + k as u32, kind: Kind::Fox.k(), x: q.x, y: q.y, approaching: d2(q.x, q.y, p.x, p.y) < d2(q.px, q.py, p.x, p.y), available: false });
                }
            }
        }
        let hunger2 = self.village.as_ref().map_or(0, |v| v.hunger[i]);
        Sense { tick: self.tick, seed: self.seed, body: Body { x: p.x, y: p.y, hunger: p.thirst, pain: p.pain, asleep: p.asleep && !awake, drowsy: self.drowsy(), hunger2 }, seen, cues: vec![], alarms: vec![] }
    }

    /// What boar `b` senses: people, landmarks and other boars in sight.
    pub fn sense_boar(&self, b: usize) -> Sense {
        let q = &self.boars[b];
        let near = |x: i32, y: i32| d2(x, y, q.x, q.y) <= SIGHT * SIGHT;
        let mut seen = vec![];
        if !q.asleep {
            for t in self.things.iter().filter(|t| t.alive && near(t.x, t.y)) { seen.push(Seen { id: t.id, kind: t.kind.k(), x: t.x, y: t.y, approaching: false, available: false }); }
            for (j, p) in self.people.iter().enumerate().filter(|(_, p)| !p.away && near(p.x, p.y)) {
                seen.push(Seen { id: person_id(j), kind: person_kind(j), x: p.x, y: p.y, approaching: d2(p.x, p.y, q.x, q.y) < d2(p.px, p.py, q.x, q.y), available: false });
            }
            for (c, o) in self.boars.iter().enumerate().filter(|&(c, o)| c != b && near(o.x, o.y)) {
                seen.push(Seen { id: BOAR_ID + c as u32, kind: Kind::Boar.k(), x: o.x, y: o.y, approaching: false, available: false });
            }
        }
        let hunger = self.village.as_ref().and_then(|v| v.boar_hunger.get(b).copied()).unwrap_or(0);
        if self.village.is_some() { for s in seen.iter_mut() { if s.kind == Kind::Berry.k() { s.available = self.servings(s.id) > 0; } } }
        Sense { tick: self.tick, seed: self.seed, body: Body { x: q.x, y: q.y, hunger, pain: q.pain, asleep: q.asleep, drowsy: 0, hunger2: 0 }, seen, cues: vec![], alarms: vec![] }
    }

    /// What deer `k` senses: people (not those hidden from it), landmarks and other deer in sight.
    pub fn sense_deer(&self, k: usize) -> Sense {
        let q = &self.deer[k];
        let near = |x: i32, y: i32| d2(x, y, q.x, q.y) <= SIGHT * SIGHT;
        let mut seen = vec![];
        if !q.asleep && q.caught.is_none() && q.escaped.is_none() {
            for t in self.things.iter().filter(|t| t.alive && near(t.x, t.y)) { seen.push(Seen { id: t.id, kind: t.kind.k(), x: t.x, y: t.y, approaching: false, available: false }); }
            for (j, p) in self.people.iter().enumerate().filter(|&(j, p)| !p.away && near(p.x, p.y) && !self.hidden(j, q.x, q.y)) {
                seen.push(Seen { id: person_id(j), kind: person_kind(j), x: p.x, y: p.y, approaching: d2(p.x, p.y, q.x, q.y) < d2(p.px, p.py, q.x, q.y), available: false });
            }
            for (c, o) in self.deer.iter().enumerate().filter(|&(c, o)| c != k && o.caught.is_none() && o.escaped.is_none() && near(o.x, o.y)) {
                seen.push(Seen { id: DEER_ID + c as u32, kind: Kind::Deer.k(), x: o.x, y: o.y, approaching: false, available: false });
            }
        }
        Sense { tick: self.tick, seed: self.seed, body: Body { x: q.x, y: q.y, hunger: q.hunger, pain: 0, asleep: q.asleep, drowsy: 0, hunger2: 0 }, seen, cues: vec![], alarms: vec![] }
    }

    /// Stage 5: what fox `k` senses: landmarks and bushes (with fruit or not), people, boars and
    /// other foxes in sight.
    pub fn sense_fox(&self, k: usize) -> Sense {
        let v = self.village.as_ref().expect("a village");
        let q = &v.foxes[k];
        let near = |x: i32, y: i32| d2(x, y, q.x, q.y) <= SIGHT * SIGHT;
        let mut seen = vec![];
        if !q.asleep {
            for t in self.things.iter().filter(|t| t.alive && near(t.x, t.y)) {
                seen.push(Seen { id: t.id, kind: t.kind.k(), x: t.x, y: t.y, approaching: false, available: t.kind == Kind::Berry && self.servings(t.id) > 0 });
            }
            for (j, p) in self.people.iter().enumerate().filter(|(_, p)| !p.away && near(p.x, p.y)) {
                seen.push(Seen { id: person_id(j), kind: person_kind(j), x: p.x, y: p.y, approaching: d2(p.x, p.y, q.x, q.y) < d2(p.px, p.py, q.x, q.y), available: false });
            }
            for (b, o) in self.boars.iter().enumerate().filter(|(_, o)| near(o.x, o.y)) {
                seen.push(Seen { id: BOAR_ID + b as u32, kind: Kind::Boar.k(), x: o.x, y: o.y, approaching: d2(o.x, o.y, q.x, q.y) < d2(o.px, o.py, q.x, q.y), available: false });
            }
            for (c, o) in v.foxes.iter().enumerate().filter(|&(c, o)| c != k && near(o.x, o.y)) {
                seen.push(Seen { id: FOX_ID + c as u32, kind: Kind::Fox.k(), x: o.x, y: o.y, approaching: false, available: false });
            }
        }
        Sense { tick: self.tick, seed: self.seed, body: Body { x: q.x, y: q.y, hunger: q.hunger, pain: 0, asleep: q.asleep, drowsy: 0, hunger2: 0 }, seen, cues: vec![], alarms: vec![] }
    }

    /// The utterances in the air that person `i` catches this tick: (utterance, loudness,
    /// addressed to me). A hashed roll decides the in-between loudnesses.
    pub fn heard(&self, i: usize) -> Vec<(Utterance, i32, bool)> {
        let p = &self.people[i];
        if p.asleep { return vec![]; }
        if self.village.as_ref().is_some_and(|v| v.voice) { return self.heard_voice(i); }
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

    /// Stage 5, the voice model: each utterance's level here (`voice_level`) against the noise,
    /// which is the background or the loudest other voice here, whichever is louder (one attends to
    /// one voice; others mask it). Caught for certain at `SNR_CLEAR`, never below `SNR_MIN`, a
    /// hashed roll between. The loudness passed on is the old scale's (`CLEAR` when clear), so the
    /// layers that read it are unchanged.
    pub fn heard_voice(&self, i: usize) -> Vec<(Utterance, i32, bool)> {
        let p = &self.people[i];
        let lv: Vec<i32> = self.air.iter().map(|u| if u.speaker == i { i32::MIN } else { voice_level(self, u, p.x, p.y) }).collect();
        let mut out = vec![];
        for (k, u) in self.air.iter().enumerate() {
            if u.speaker == i { continue; }
            let masker = lv.iter().enumerate().filter(|&(j, _)| j != k).map(|(_, &l)| l).max().unwrap_or(i32::MIN).max(AMBIENT);
            let snr = lv[k] - masker;
            if snr < SNR_MIN { continue; }
            if snr < SNR_CLEAR {
                let catch = (snr - SNR_MIN) * 1000 / (SNR_CLEAR - SNR_MIN);
                if ((hash(self.seed, self.tick as u64, i as u64, 0x4ea3 ^ (k as u64) << 8 ^ u.speaker as u64) % 1000) as i32) >= catch { continue; }
            }
            let loud = HEAR_MIN + (snr - SNR_MIN).clamp(0, SNR_CLEAR - SNR_MIN) * (CLEAR - HEAR_MIN) / (SNR_CLEAR - SNR_MIN);
            out.push((u.clone(), loud, u.to == Some(i)));
        }
        out
    }
    /// Stage 5: berries left on a bush (0 for anything else, or outside the village).
    pub fn servings(&self, id: u32) -> i32 { self.village.as_ref().and_then(|v| v.bushes.iter().find(|b| b.id == id)).map_or(0, |b| b.servings) }

    /// Apply the script. Call before minds read the world.
    pub fn pre_step(&mut self) {
        self.night_now = false;
        for p in self.people.iter_mut() { p.px = p.x; p.py = p.y; }
        for q in self.boars.iter_mut() { q.px = q.x; q.py = q.y; }
        for q in self.deer.iter_mut() { q.px = q.x; q.py = q.y; }
        if let Some(v) = &mut self.village { for q in v.foxes.iter_mut() { q.px = q.x; q.py = q.y; } }
        let evs: Vec<Event> = self.script.iter().filter(|(t, _)| *t == self.tick).map(|(_, e)| e.clone()).collect();
        for ev in evs {
            match ev {
                Event::Dusk => { self.dusk = true; self.dusk_at = self.tick; }
                Event::Night => { self.dusk = false; self.night_now = true; for p in self.people.iter_mut() { p.asleep = true; } for q in self.boars.iter_mut() { q.asleep = true; } for q in self.deer.iter_mut() { q.asleep = true; }
                    if let Some(v) = &mut self.village {
                        for q in v.foxes.iter_mut() { q.asleep = true; }
                        // Wounds heal overnight, one a night; a caught deer is back on its meadow.
                        if v.heal {
                            for p in self.people.iter_mut() { p.gored = p.gored.saturating_sub(1); }
                            // A boar that broke off has recovered by morning.
                            for q in self.boars.iter_mut() { q.routed = None; q.struck = 0; q.braced = 0; q.pain = 0; }
                        }
                        for (k, q) in self.deer.iter_mut().enumerate() { if q.caught.is_some() { if let Some(&(x, y)) = v.deer_home.get(k) { q.caught = None; q.x = x; q.y = y; q.px = x; q.py = y; q.hunger = 300; } } }
                    }
                    self.log.push((self.tick, "night: everyone sleeps where they are".into())); }
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
                    self.log.push((self.tick, format!("{} drops a note without noticing", name(who))));
                }
                Event::Away { who } => { let p = &mut self.people[who]; p.away = true; p.x = 63; p.y = 39; p.px = 63; p.py = 39; }
                Event::Back { who, x, y } => { let p = &mut self.people[who]; p.away = false; p.x = x; p.y = y; p.px = x; p.py = y; }
                Event::PutBoar { b, x, y } => { let q = &mut self.boars[b]; q.x = x; q.y = y; q.px = x; q.py = y; }
                Event::MoveNote { id, x, y } => { if let Some(n) = self.notes.iter_mut().find(|n| n.id == id) { n.holder = None; n.x = x; n.y = y; } }
            }
        }
    }

    /// Apply every person's deed (in person order), their bodies' needs, and the sounds they
    /// made (which are heard next tick). Then advance the tick.
    pub fn post_step(&mut self, deeds: &[Deed]) { self.post_step_with(deeds, &[]); }
    /// `post_step` with what each boar does (stage 3). People's deeds are applied first, then the
    /// boars'; every choice was made from the world as it was at the start of the tick.
    pub fn post_step_with(&mut self, deeds: &[Deed], boar_acts: &[Action]) { self.post_step_all(deeds, boar_acts, &[]); }
    /// Stage 4: with what each deer does too (applied after the boars').
    pub fn post_step_all(&mut self, deeds: &[Deed], boar_acts: &[Action], deer_acts: &[Action]) { self.post_step_full(deeds, boar_acts, deer_acts, &[]); }
    /// Stage 5: with what each fox does too (applied after the deer's).
    pub fn post_step_full(&mut self, deeds: &[Deed], boar_acts: &[Action], deer_acts: &[Action], fox_acts: &[Action]) {
        let tick = self.tick;
        let n = self.people.len();
        let mut air = vec![];
        for (i, d) in deeds.iter().enumerate() {
            if let Some(a) = d.action.filter(|_| self.people[i].gored < DOWN_AFTER) {
                let p = &mut self.people[i];
                match a {
                    Action::WalkTo { x, y } => { let (nx, ny) = step_toward(p.x, p.y, x, y); p.x = nx.clamp(0, GRID_W - 1); p.y = ny.clamp(0, GRID_H - 1); }
                    Action::Flee { from_x, from_y, speed } => {
                        // Stage 3: a frightened person runs (2 cells a tick at full speed).
                        for _ in 0..if speed >= 3 { 2 } else { 1 } { let (nx, ny) = step_toward(p.x, p.y, 2 * p.x - from_x, 2 * p.y - from_y); p.x = nx.clamp(0, GRID_W - 1); p.y = ny.clamp(0, GRID_H - 1); }
                    }
                    Action::Charge { x, y, .. } => { let (nx, ny) = step_toward(p.x, p.y, x, y); p.x = nx.clamp(0, GRID_W - 1); p.y = ny.clamp(0, GRID_H - 1); }
                    Action::Snap { entity } if entity >= BOAR_ID && ((entity - BOAR_ID) as usize) < self.boars.len() => {
                        let (px, py) = (p.x, p.y);
                        let q = &mut self.boars[(entity - BOAR_ID) as usize];
                        if (q.x - px).abs().max((q.y - py).abs()) <= 2 { q.pain = (q.pain + STRIKE).min(1000); q.struck += 1; self.log.push((tick, format!("{} strikes the boar", name(i)))); }
                    }
                    // Stage 4: a strike at a deer, or at another person.
                    Action::Snap { entity } if entity >= DEER_ID && ((entity - DEER_ID) as usize) < self.deer.len() => {
                        let (px, py) = (p.x, p.y);
                        let k = (entity - DEER_ID) as usize;
                        let roll = (hash(self.seed, tick as u64, i as u64, 0xdee7) % 1000) as i32;
                        let q = &mut self.deer[k];
                        if q.caught.is_none() && q.escaped.is_none() && (q.x - px).abs().max((q.y - py).abs()) <= 2 {
                            if roll < HIT {
                                q.caught = Some((tick, i)); self.log.push((tick, format!("{} catches the deer", name(i))));
                                // Stage 5: a caught deer feeds everyone near it (a world rule).
                                let (dx, dy) = (q.x, q.y);
                                if let Some(v) = &mut self.village { for (j, o) in self.people.iter().enumerate() { if (o.x - dx).abs().max((o.y - dy).abs()) <= FEAST_R { v.hunger[j] = (v.hunger[j] - FEAST).max(0); v.feasts.push((tick, j as u32)); } } }
                            } else { q.missed += 1; self.log.push((tick, format!("{} strikes at the deer and misses", name(i)))); }
                        }
                    }
                    Action::Snap { entity } if person_of(entity, n).is_some_and(|j| j != i) => {
                        let j = person_of(entity, n).unwrap();
                        let (px, py) = (p.x, p.y);
                        let q = &mut self.people[j];
                        // Three blows and a person is down (counted with gorings: wounds).
                        if !q.away && q.gored < DOWN_AFTER && (q.x - px).abs().max((q.y - py).abs()) <= 2 { q.pain = (q.pain + STRIKE).min(1000); q.gored += 1; self.blows.push((tick, i, j)); self.log.push((tick, format!("{} strikes {}", name(i), name(j)))); }
                    }
                    Action::Eat { entity } if self.village.is_some() && self.things.iter().any(|t| t.id == entity && t.kind == Kind::Berry) => {
                        let (px, py) = (p.x, p.y);
                        let near = self.things.iter().any(|t| t.id == entity && t.alive && (t.x - px).abs().max((t.y - py).abs()) <= 1);
                        let v = self.village.as_mut().unwrap();
                        if let Some(b) = v.bushes.iter_mut().find(|b| b.id == entity && b.servings > 0).filter(|_| near) {
                            b.servings -= 1; v.hunger[i] = (v.hunger[i] - MEAL).max(0); v.meals[i] += 1;
                        }
                    }
                    Action::Eat { entity } => {
                        let (px, py) = (p.x, p.y);
                        if self.things.iter().any(|t| t.id == entity && t.alive && t.kind == Kind::Water && (t.x - px).abs().max((t.y - py).abs()) <= 1) {
                            let p = &mut self.people[i];
                            p.thirst = (p.thirst - 500).max(0); p.drinks += 1;
                            if p.first_drink.is_none() { p.first_drink = Some((tick, entity)); }
                            self.log.push((tick, format!("{} drinks from {}", name(i), self.thing(entity).name)));
                        }
                    }
                    _ => {}
                }
            }
            if let Some((say, to, vol)) = &d.say {
                let p = &self.people[i];
                let u = Utterance { tick, speaker: i, x: p.x, y: p.y, vol: *vol, to: *to, say: say.clone() };
                self.log.push((tick, format!("{} says to {}: {}", name(i), to.map(name).unwrap_or("anyone"), u.say.render())));
                air.push(u);
            }
            if let Some((content, to)) = &d.write {
                let id = self.next_id; self.next_id += 1;
                let (x, y) = (self.people[*to].x, self.people[*to].y);
                self.notes.push(Note { id, x, y, holder: Some(*to), content: content.clone(), writer: i });
                self.log.push((tick, format!("{} writes a note and hands it to {}", name(i), name(*to))));
            }
            if let Some(id) = d.pick_up {
                let (x, y) = (self.people[i].x, self.people[i].y);
                if let Some(n) = self.notes.iter_mut().find(|n| n.id == id && n.holder.is_none() && (n.x - x).abs().max((n.y - y).abs()) <= 1) {
                    n.holder = Some(i);
                    self.log.push((tick, format!("{} picks up a note", name(i))));
                }
            }
        }
        // Boars act after people.
        for (b, a) in boar_acts.iter().enumerate() {
            let q = &mut self.boars[b];
            if q.routed.is_none() && q.struck + q.braced >= ROUT_AFTER { q.routed = Some((q.x, q.y)); self.log.push((tick, "the boar, badly hurt, breaks off".into())); }
            let a = match q.routed { Some((fx, fy)) => &Action::Flee { from_x: fx, from_y: fy, speed: 3 }, None => a };
            match *a {
                Action::WalkTo { x, y } => { let (nx, ny) = step_toward(q.x, q.y, x, y); q.x = nx.clamp(0, GRID_W - 1); q.y = ny.clamp(0, GRID_H - 1); }
                Action::Charge { x, y, .. } => { for _ in 0..2 { if (q.x - x).abs().max((q.y - y).abs()) <= 1 { break; } let (nx, ny) = step_toward(q.x, q.y, x, y); q.x = nx.clamp(0, GRID_W - 1); q.y = ny.clamp(0, GRID_H - 1); } }
                Action::Flee { from_x, from_y, .. } => { for _ in 0..2 { let (nx, ny) = step_toward(q.x, q.y, 2 * q.x - from_x, 2 * q.y - from_y); q.x = nx.clamp(0, GRID_W - 1); q.y = ny.clamp(0, GRID_H - 1); } }
                Action::Eat { entity } => {
                    // Stage 5: a village boar eats from a bush within a cell.
                    let (bx, by) = (q.x, q.y);
                    let near = self.things.iter().any(|t| t.id == entity && t.alive && t.kind == Kind::Berry && (t.x - bx).abs().max((t.y - by).abs()) <= 1);
                    if let Some(v) = &mut self.village {
                        if let Some(bu) = v.bushes.iter_mut().find(|bu| bu.id == entity && bu.servings > 0).filter(|_| near) { bu.servings -= 1; if let Some(h) = v.boar_hunger.get_mut(b) { *h = (*h - MEAL).max(0); } if let Some(m) = v.boar_meals.get_mut(b) { *m += 1; } }
                    }
                }
                Action::Snap { entity } if person_of(entity, n).is_some() => {
                    let j = person_of(entity, n).unwrap();
                    let (qx, qy) = (q.x, q.y);
                    let p = &mut self.people[j];
                    if (p.x - qx).abs().max((p.y - qy).abs()) <= 2 && !p.away && p.gored < DOWN_AFTER {
                        let braced = matches!(deeds.get(j).and_then(|d| d.action), Some(Action::Hold { entity: e }) if e == BOAR_ID + b as u32);
                        // (After review: a braced charge no longer counts as a goring: bracing is
                        // what protects the holder; it bruises, and hurts the boar.)
                        if !braced { p.gored += 1; }
                        if braced { p.pain = (p.pain + BRACED_GORE).min(1000); q.pain = (q.pain + BRACE).min(1000); q.braced += 1; self.log.push((tick, format!("the boar charges {}, who holds braced", name(j)))); }
                        else { p.pain = (p.pain + GORE).min(1000); q.gored += 1; self.log.push((tick, format!("the boar gores {}", name(j)))); }
                    }
                }
                _ => {}
            }
        }
        for q in self.boars.iter_mut() { q.pain -= q.pain / 10; if q.asleep { q.asleep = false; } }
        // Deer act after boars.
        for (k, a) in deer_acts.iter().enumerate() {
            let q = &mut self.deer[k];
            if q.caught.is_some() || q.escaped.is_some() { continue; }
            let (x0, y0) = (q.x, q.y);
            match *a {
                Action::WalkTo { x, y } => { let (nx, ny) = step_toward(q.x, q.y, x, y); q.x = nx.clamp(0, GRID_W - 1); q.y = ny.clamp(0, GRID_H - 1); }
                // A fleeing deer takes, twice, the neighbouring cell furthest from what it flees
                // (so at a wall it runs along it instead of stopping).
                Action::Flee { from_x, from_y, .. } => { for _ in 0..2 { let (nx, ny) = flee_step(q.x, q.y, from_x, from_y); q.x = nx; q.y = ny; } }
                _ => {}
            }
            if (q.x - x0).abs().max((q.y - y0).abs()) >= 2 { q.ran += 1; }

            let grazing = (q.x, q.y) == (x0, y0) && self.meadows.iter().any(|&(mx, my)| (mx - q.x).abs().max((my - q.y).abs()) <= GRAZE_R);
            q.hunger = if grazing { (q.hunger - GRAZE).max(0) } else { (q.hunger + 2).min(1000) };
            if q.asleep { q.asleep = false; }
        }
        // Foxes act after deer (stage 5): walk, run, or eat from a bush within a cell.
        if let Some(v) = &mut self.village {
            for (k, a) in fox_acts.iter().enumerate() {
                let q = &mut v.foxes[k];
                match *a {
                    Action::WalkTo { x, y } => { let (nx, ny) = step_toward(q.x, q.y, x, y); q.x = nx.clamp(0, GRID_W - 1); q.y = ny.clamp(0, GRID_H - 1); }
                    Action::Flee { from_x, from_y, .. } => { for _ in 0..2 { let (nx, ny) = flee_step(q.x, q.y, from_x, from_y); q.x = nx; q.y = ny; } }
                    Action::Eat { entity } => {
                        let (fx, fy) = (q.x, q.y);
                        let near = self.things.iter().any(|t| t.id == entity && t.alive && t.kind == Kind::Berry && (t.x - fx).abs().max((t.y - fy).abs()) <= 1);
                        if let Some(b) = v.bushes.iter_mut().find(|b| b.id == entity && b.servings > 0).filter(|_| near) { b.servings -= 1; q.hunger = (q.hunger - MEAL).max(0); q.meals += 1; }
                    }
                    _ => {}
                }
            }
            for q in v.foxes.iter_mut() { q.hunger = (q.hunger + 2).min(1000); if q.asleep { q.asleep = false; } }
        }
        if self.cover {
            self.still.resize(self.people.len(), 0);
            for (j, p) in self.people.iter().enumerate() { if (p.x, p.y) == (p.px, p.py) { self.still[j] += 1; } else { self.still[j] = 0; } }
        }
        // Being led: walk beside the leader (a demonstration), whatever the mind chose.
        self.leads.retain(|l| l.2 > tick);
        for &(who, by, _) in &self.leads.clone() {
            let (lx, ly) = (self.people[by].x, self.people[by].y);
            let p = &mut self.people[who];
            p.x = (lx - 1).max(0); p.y = ly;
        }
        if let Some(v) = &mut self.village {
            for h in v.hunger.iter_mut() { *h = (*h + HUNGER_RATE).min(1000); }
            for h in v.boar_hunger.iter_mut() { *h = (*h + 2).min(1000); }
            for b in v.bushes.iter_mut() { if b.regrow > 0 && tick % b.regrow == 0 { b.servings = (b.servings + 1).min(b.max); } }
        }
        for (i, p) in self.people.iter_mut().enumerate() {
            if p.asleep { p.asleep = false; }
            p.thirst = (p.thirst + 2).min(1000);
            p.pain -= p.pain / 10;
            for n in self.notes.iter_mut().filter(|n| n.holder == Some(i)) { n.x = p.x; n.y = p.y; }
        }
        self.said.extend(air.iter().cloned());
        self.air = air;
        self.tick += 1;
    }

    pub fn hash_into<H: Hasher>(&self, h: &mut H) {
        self.tick.hash(h); self.things.hash(h); self.notes.hash(h); self.people.hash(h); self.air.hash(h); self.leads.hash(h); self.dusk.hash(h); self.dusk_at.hash(h); self.boars.hash(h);
        // Stage 4's state, only when in use (so earlier stages' world hashes don't move).
        if !self.deer.is_empty() || self.cover || !self.blows.is_empty() {
            self.deer.hash(h); self.meadows.hash(h); self.cover.hash(h);
            self.still.len().hash(h); for &x in &self.still { x.hash(h); }
            self.blows.len().hash(h); for &(t, a, b) in &self.blows { t.hash(h); (a as u64).hash(h); (b as u64).hash(h); }
        }
        if let Some(v) = &self.village { v.hash(h); }
    }
    pub fn state_hash(&self) -> u64 { let mut h = StateHasher::new(self.seed); self.hash_into(&mut h); h.finish() }
}
