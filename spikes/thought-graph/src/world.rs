//! A small grounded world: a 64 × 40 grid of 1 m cells, six places, a few things, a dog and a fox.
//! Integer coordinates and integer maths only. The world knows nothing about minds: the fox's
//! mind reads a view of it and returns one action, which the world applies.

use crate::rng::StateHasher;
use std::hash::{Hash, Hasher};

pub const GRID_W: i32 = 64;
pub const GRID_H: i32 = 40;
/// The fox sees things within this many cells (a circle; no line-of-sight test).
pub const SIGHT: i32 = 10;
/// A thing within this many cells of a place is "at" that place.
pub const PLACE_RADIUS: i32 = 6;
/// The dog notices the fox within this many cells of itself.
pub const DOG_NOTICE: i32 = 8;

/// The kinds of thing a fox can perceive.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Kind { Dog, Hen, Berry, Water, Fire, Den }
impl Kind {
    pub const ALL: [Kind; 6] = [Kind::Dog, Kind::Hen, Kind::Berry, Kind::Water, Kind::Fire, Kind::Den];
    pub fn idx(self) -> usize { self as usize }
    pub fn name(self) -> &'static str {
        match self { Kind::Dog => "dog", Kind::Hen => "hen", Kind::Berry => "berry", Kind::Water => "water", Kind::Fire => "fire", Kind::Den => "den" }
    }
    /// Landmarks are fixed things that define a place: seeing one where the mind knows no place
    /// makes it learn a new place. Creatures move and fires come and go, so they are not.
    pub fn landmark(self) -> bool { matches!(self, Kind::Hen | Kind::Berry | Kind::Water | Kind::Den) }
}

#[derive(Clone, Debug, Hash)]
pub struct Entity {
    pub id: u32,
    pub kind: Kind,
    pub name: String,
    pub x: i32,
    pub y: i32,
    /// Food left (hens are plentiful; the bush holds a few servings a day).
    pub servings: i32,
    pub alive: bool,
    /// Moved closer to the fox this tick (only the dog moves).
    pub approaching: bool,
}

#[derive(Clone, Debug, Hash)]
pub struct Place { pub name: &'static str, pub x: i32, pub y: i32, pub home: bool }

#[derive(Clone, Debug, Hash)]
pub struct Fox { pub x: i32, pub y: i32, pub hunger: i32, pub fatigue: i32, pub pain: i32, pub asleep: bool }

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum DogMode { Guard, Chase { ticks: i32 }, Return }

/// Things a scenario can make happen at a tick.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum Event {
    /// A fire appears at a cell (a new entity the fox has never seen).
    FireAt { x: i32, y: i32 },
    /// Every fire that isn't the campfire goes out.
    FiresOut,
    /// The dog starts chasing the fox wherever it is.
    DogChase,
    /// The dog is taken away (control runs).
    NoDog,
    /// Night: the fox is put where its *mind* believes home is (the driver does this; it doesn't
    /// walk there, see README) and sleeps.
    Night,
    SetHunger(i32),
    /// The supernatural events of the home experiment. The fox's burrow (the earth it sleeps in)
    /// is moved to a cell, removed, or joined by a second, identical-looking burrow. The world's
    /// own "den" label moves with the original burrow; the fox's mind is not told anything.
    MoveDen { x: i32, y: i32 },
    RemoveDen,
    AddDen { x: i32, y: i32 },
}

/// What the fox does this tick.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum Action {
    Stay,
    /// Walk one cell toward a point.
    WalkTo { x: i32, y: i32 },
    /// Run directly away from a point, `speed` cells a tick (2, or 3 when stressed).
    Flee { from_x: i32, from_y: i32, speed: i32 },
    /// Stand and snap at a creature within two cells. A snapped-at dog backs off.
    Snap { entity: u32 },
    /// Eat from an entity within one cell.
    Eat { entity: u32 },
    Sleep,
}

#[derive(Clone, Debug)]
pub struct World {
    pub seed: u64,
    pub tick: u32,
    pub places: Vec<Place>,
    pub entities: Vec<Entity>,
    pub fox: Fox,
    pub dog_mode: DogMode,
    pub dog_home: (i32, i32),
    pub script: Vec<(u32, Event)>,
    pub log: Vec<(u32, String)>,
    pub next_id: u32,
    /// Set when the Night event fires, so the driver can run the mind's sleep.
    pub night_now: bool,
}

pub const DOG: u32 = 1;
/// The fox's burrow: the physical earth at its den.
pub const BURROW: u32 = 6;

pub fn d2(ax: i32, ay: i32, bx: i32, by: i32) -> i32 { (ax - bx) * (ax - bx) + (ay - by) * (ay - by) }
pub fn dist(ax: i32, ay: i32, bx: i32, by: i32) -> i32 { (d2(ax, ay, bx, by) as u32).isqrt() as i32 }

/// One step on the 8-neighbour grid toward a target, roughly along the straight line.
pub fn step_toward(x: i32, y: i32, tx: i32, ty: i32) -> (i32, i32) {
    let (dx, dy) = (tx - x, ty - y);
    let (sx, sy) = (dx.signum(), dy.signum());
    if dx.abs() > 2 * dy.abs() { (x + sx, y) } else if dy.abs() > 2 * dx.abs() { (x, y + sy) } else { (x + sx, y + sy) }
}

fn clamp_grid(x: i32, y: i32) -> (i32, i32) { (x.clamp(0, GRID_W - 1), y.clamp(0, GRID_H - 1)) }

impl World {
    /// The valley used by every scenario. Coordinates are in cells (1 m).
    pub fn valley(seed: u64) -> World {
        let places = vec![
            Place { name: "den", x: 8, y: 30, home: true },
            Place { name: "stream", x: 12, y: 16, home: false },
            Place { name: "henhouse", x: 22, y: 8, home: false },
            Place { name: "berry bush", x: 52, y: 8, home: false },
            Place { name: "camp", x: 22, y: 26, home: false },
            Place { name: "meadow", x: 34, y: 17, home: false },
        ];
        let e = |id, kind, name: &str, x, y, servings| Entity { id, kind, name: name.into(), x, y, servings, alive: true, approaching: false };
        let entities = vec![
            e(DOG, Kind::Dog, "the dog", 24, 10, 0),
            e(2, Kind::Hen, "the hens", 22, 7, 99),
            e(3, Kind::Berry, "the berries", 52, 9, 6),
            e(4, Kind::Water, "the stream water", 12, 17, 0),
            e(5, Kind::Fire, "the campfire", 23, 26, 0),
            e(BURROW, Kind::Den, "the burrow", 8, 31, 0),
        ];
        World {
            seed, tick: 0, places, entities,
            fox: Fox { x: 8, y: 30, hunger: 300, fatigue: 0, pain: 0, asleep: false },
            dog_mode: DogMode::Guard, dog_home: (24, 10), script: vec![], log: vec![], next_id: 100, night_now: false,
        }
    }

    pub fn place_index(&self, name: &str) -> usize { self.places.iter().position(|p| p.name == name).expect("place") }

    /// The place a point is at: the nearest within `PLACE_RADIUS`, ties to the lower index.
    pub fn place_at(&self, x: i32, y: i32) -> Option<usize> {
        let mut best: Option<(i32, usize)> = None;
        for (i, p) in self.places.iter().enumerate() {
            let d = d2(x, y, p.x, p.y);
            if d <= PLACE_RADIUS * PLACE_RADIUS && best.map_or(true, |(bd, _)| d < bd) { best = Some((d, i)); }
        }
        best.map(|b| b.1)
    }

    pub fn entity(&self, id: u32) -> Option<&Entity> { self.entities.iter().find(|e| e.id == id) }
    fn entity_mut(&mut self, id: u32) -> Option<&mut Entity> { self.entities.iter_mut().find(|e| e.id == id) }

    /// What the fox can see: living entities within `SIGHT`, in id order.
    pub fn visible(&self) -> Vec<&Entity> {
        let f = &self.fox;
        if f.asleep { return vec![]; }
        self.entities.iter().filter(|e| e.alive && d2(e.x, e.y, f.x, f.y) <= SIGHT * SIGHT).collect()
    }

    /// Apply the script and move the dog. Call before the mind reads the world.
    pub fn pre_step(&mut self) {
        self.night_now = false;
        let evs: Vec<Event> = self.script.iter().filter(|(t, _)| *t == self.tick).map(|(_, e)| e.clone()).collect();
        for ev in evs {
            match ev {
                Event::FireAt { x, y } => {
                    let id = self.next_id; self.next_id += 1;
                    self.entities.push(Entity { id, kind: Kind::Fire, name: "a fire".into(), x, y, servings: 0, alive: true, approaching: false });
                    self.log.push((self.tick, format!("a fire breaks out at ({x}, {y})")));
                }
                Event::FiresOut => {
                    for e in self.entities.iter_mut().filter(|e| e.kind == Kind::Fire && e.id >= 100) { e.alive = false; }
                    self.log.push((self.tick, "the fire goes out".into()));
                }
                Event::DogChase => { self.dog_mode = DogMode::Chase { ticks: 0 }; self.log.push((self.tick, "the dog gives chase".into())); }
                Event::NoDog => { if let Some(d) = self.entity_mut(DOG) { d.alive = false; } }
                Event::Night => {
                    self.fox.asleep = true; self.night_now = true;
                    for e in self.entities.iter_mut().filter(|e| e.kind == Kind::Berry) { e.servings = 6; }
                    self.log.push((self.tick, "night: the fox sleeps where it believes home is".into()));
                }
                Event::SetHunger(h) => self.fox.hunger = h,
                Event::MoveDen { x, y } => {
                    if let Some(b) = self.entity_mut(BURROW) { b.x = x; b.y = y + 1; }
                    if let Some(p) = self.places.iter_mut().find(|p| p.home) { p.x = x; p.y = y; }
                    self.log.push((self.tick, format!("the burrow is moved to ({x}, {y})")));
                }
                Event::RemoveDen => {
                    if let Some(b) = self.entity_mut(BURROW) { b.alive = false; }
                    self.log.push((self.tick, "the burrow vanishes".into()));
                }
                Event::AddDen { x, y } => {
                    let id = self.next_id; self.next_id += 1;
                    self.entities.push(Entity { id, kind: Kind::Den, name: "a second burrow".into(), x, y: y + 1, servings: 0, alive: true, approaching: false });
                    self.log.push((self.tick, format!("a second burrow appears at ({x}, {y})")));
                }
            }
        }
        self.move_dog();
    }

    fn move_dog(&mut self) {
        let (fx, fy) = (self.fox.x, self.fox.y);
        let home = self.dog_home;
        let mode = self.dog_mode;
        let tick = self.tick;
        let Some(d) = self.entities.iter_mut().find(|e| e.id == DOG && e.alive) else { return };
        let before = d2(d.x, d.y, fx, fy);
        let mut bite = false;
        let mut mode2 = mode;
        match mode {
            DogMode::Guard => {
                if !self.fox.asleep && d2(d.x, d.y, fx, fy) <= DOG_NOTICE * DOG_NOTICE { mode2 = DogMode::Chase { ticks: 0 }; }
            }
            DogMode::Chase { ticks } => {
                for _ in 0..3 {
                    if (d.x - fx).abs().max((d.y - fy).abs()) <= 1 { break; }
                    let (nx, ny) = step_toward(d.x, d.y, fx, fy); d.x = nx; d.y = ny;
                }
                if (d.x - fx).abs().max((d.y - fy).abs()) <= 1 { bite = true; mode2 = DogMode::Return; }
                else if ticks >= 12 || d2(d.x, d.y, fx, fy) > 16 * 16 { mode2 = DogMode::Return; }
                else { mode2 = DogMode::Chase { ticks: ticks + 1 }; }
            }
            DogMode::Return => {
                let (nx, ny) = step_toward(d.x, d.y, home.0, home.1); d.x = nx; d.y = ny;
                if (d.x, d.y) == home { mode2 = DogMode::Guard; }
            }
        }
        d.approaching = d2(d.x, d.y, fx, fy) < before;
        self.dog_mode = mode2;
        if bite {
            self.fox.pain = (self.fox.pain + 600).min(1000);
            self.log.push((tick, "the dog bites the fox".into()));
        }
    }

    /// Apply the fox's action and its body's needs, then advance the tick.
    pub fn post_step(&mut self, a: Action) {
        let f = &mut self.fox;
        let mut moved = 0;
        match a {
            Action::Stay => {}
            Action::WalkTo { x, y } => { let (nx, ny) = step_toward(f.x, f.y, x, y); f.x = nx; f.y = ny; moved = 1; }
            Action::Snap { entity } => {
                let (fx, fy) = (f.x, f.y);
                if entity == DOG {
                    if let Some(d) = self.entities.iter().find(|e| e.id == DOG && e.alive) {
                        if (d.x - fx).abs().max((d.y - fy).abs()) <= 2 { self.dog_mode = DogMode::Return; }
                    }
                }
                self.log.push((self.tick, "the fox snaps at the dog".into()));
            }
            Action::Flee { from_x, from_y, speed } => {
                for _ in 0..speed {
                    let (dx, dy) = (f.x - from_x, f.y - from_y);
                    let (tx, ty) = if dx == 0 && dy == 0 { (f.x - 1, f.y + 1) } else { (f.x + dx.signum() * 8, f.y + dy.signum() * 8) };
                    let (nx, ny) = step_toward(f.x, f.y, tx, ty);
                    let (nx, ny) = clamp_grid(nx, ny); f.x = nx; f.y = ny;
                }
                moved = speed;
            }
            Action::Eat { entity } => {
                let (fx, fy) = (f.x, f.y);
                let tick = self.tick;
                if let Some(e) = self.entities.iter_mut().find(|e| e.id == entity && e.alive) {
                    if (e.x - fx).abs().max((e.y - fy).abs()) <= 1 && e.servings > 0 {
                        let amount = match e.kind { Kind::Hen => 500, Kind::Berry => 250, _ => 0 };
                        e.servings -= 1;
                        self.fox.hunger = (self.fox.hunger - amount).max(0);
                        self.log.push((tick, format!("the fox eats from {}", e.name)));
                    }
                }
            }
            Action::Sleep => {}
        }
        let f = &mut self.fox;
        if f.asleep { f.fatigue = 0; f.asleep = false; } else { f.fatigue = (f.fatigue + 1 + moved).min(1000); }
        f.hunger = (f.hunger + 2).min(1000);
        f.pain -= f.pain / 20 + (f.pain > 0) as i32;
        f.pain = f.pain.max(0);
        self.tick += 1;
    }

    pub fn hash_into<H: Hasher>(&self, h: &mut H) {
        self.tick.hash(h);
        self.entities.hash(h);
        self.fox.hash(h);
        self.dog_mode.hash(h);
    }

    pub fn state_hash(&self) -> u64 { let mut h = StateHasher::new(self.seed); self.hash_into(&mut h); h.finish() }
}
