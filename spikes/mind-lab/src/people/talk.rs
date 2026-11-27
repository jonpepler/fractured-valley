//! What people can say or write. Content is structured, never text: English appears only in
//! logs. Words name things **in the world** (a landmark's name, a person's name), the shared
//! convention of a language; what a word *means to a mind* is that mind's own node for it, found
//! through its own lexicon. Nothing in an utterance is a node id: nodes are never shared.
//!
//! A place is described the way the speaker's own mind holds it: by the landmarks it knows near
//! it, each with a compass direction (8 points) and a distance band ("close to", "a short walk
//! from"). A drawn map instead places it among landmarks by drawn offsets (to 2 cells). The
//! listener grounds it in its own landmarks, wherever *it* believes they are. Nothing is ever
//! garbled on purpose: what drifts in a retelling falls out of each mind's own grounding.

/// A word: a landmark or a person, by its world id (the name everyone uses for it).
pub type Word = u32;

/// One spoken reference: "<band> <landmark>, towards <dir>".
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct Ref { pub landmark: Word, pub dir: u8, pub band: u8 }
/// One drawn reference: the landmark drawn at this offset from the described place, in units of
/// 2 cells (the drawing's precision).
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct Drawn { pub landmark: Word, pub dx: i8, pub dy: i8 }

/// A claim about a place: water is there (or, `dry`, no longer), described by spoken references
/// or by a drawing, with the speaker's stated confidence, and who they had it from (`via`: "Bryn
/// said Ash said").
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Content { pub refs: Vec<Ref>, pub drawn: Vec<Drawn>, pub conf: i32, pub via: Option<usize>, pub dry: bool }

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum Say {
    /// "Where can I find water?"
    AskWater,
    /// A claim about a water place.
    Tell(Content),
    /// "You know it: the water by …" (the teller thinks the listener knows it already).
    Remind(Content),
    /// "Thank you (for the water)."
    Thanks,
    /// "That water by … is dry now." (an honest teller correcting itself).
    Correct(Content),
    /// "Where is the …?" about something else entirely (used to test sharing back).
    AskOther(u32),
    /// An answer to `AskOther`: "it's at …" (refs only; what it is is in the question).
    TellOther(u32, Content),
}

/// Distance bands: (largest distance in the band, the distance a listener takes it to mean,
/// half the band's width).
pub const BANDS: [(i32, i32, i32); 5] = [(3, 2, 2), (8, 5, 3), (15, 11, 4), (25, 20, 5), (99, 32, 8)];
pub const BAND_WORDS: [&str; 5] = ["right by", "close to", "a short walk from", "a walk from", "far from"];
/// The 8 compass points as unit vectors (per mille); y grows southward.
pub const DIRS: [(i32, i32); 8] = [(1000, 0), (707, -707), (0, -1000), (-707, -707), (-1000, 0), (-707, 707), (0, 1000), (707, 707)];
pub const DIR_WORDS: [&str; 8] = ["sunrise", "north-east", "north", "north-west", "sunset", "south-west", "south", "south-east"];

/// The band and compass point of an offset.
pub fn band_dir(dx: i32, dy: i32) -> (u8, u8) {
    let d = ((dx * dx + dy * dy) as u32).isqrt() as i32;
    let band = BANDS.iter().position(|b| d <= b.0).unwrap_or(4) as u8;
    let dir = (0..8).max_by_key(|&k| (DIRS[k].0 as i64 * dx as i64 + DIRS[k].1 as i64 * dy as i64, -(k as i64))).unwrap() as u8;
    (band, dir)
}
/// Where a spoken reference puts the place, from where the listener believes the landmark is,
/// and how far off that could be (band half-width plus half a compass sector at that distance).
pub fn from_ref(lx: i32, ly: i32, r: &Ref) -> (i32, i32, i32) {
    let (_, c, hw) = BANDS[r.band as usize];
    let (dx, dy) = DIRS[r.dir as usize];
    (lx + dx * c / 1000, ly + dy * c / 1000, hw + c * 383 / 1000)
}

impl Content {
    pub fn render(&self) -> String {
        let mut parts: Vec<String> = self.refs.iter().map(|r| format!("{} landmark {} (towards {})", BAND_WORDS[r.band as usize], r.landmark, DIR_WORDS[r.dir as usize])).collect();
        if !self.drawn.is_empty() { parts.push(format!("a drawing with {} landmarks", self.drawn.len())); }
        let via = self.via.map(|v| format!(" ({} said so)", super::world::NAMES[v])).unwrap_or_default();
        format!("{}water, {}{via}", if self.dry { "no " } else { "" }, parts.join(", "))
    }
}
impl Say {
    pub fn render(&self) -> String {
        match self {
            Say::AskWater => "Where can I find water?".into(),
            Say::Tell(c) => format!("There's {}.", c.render()),
            Say::Remind(c) => format!("You know it: {}.", c.render()),
            Say::Thanks => "Thank you!".into(),
            Say::Correct(c) => format!("Careful: {}, now.", c.render()),
            Say::AskOther(w) => format!("Where is thing {w}?"),
            Say::TellOther(w, c) => format!("Thing {w} is {}.", c.render()),
        }
    }
    /// The content of a claim, if this is one.
    pub fn content(&self) -> Option<&Content> {
        match self { Say::Tell(c) | Say::Remind(c) | Say::Correct(c) | Say::TellOther(_, c) => Some(c), _ => None }
    }
}
