//! A simple sound channel, outside the cell system. A voice has a volume (per mille); loudness
//! falls with distance; quiet sounds are missed or misheard, decided by a seeded roll.

use crate::model::Utt;

/// Below this loudness nothing is heard.
pub const HEAR_MIN: i32 = 60;
/// At or above this loudness everything is heard clearly.
pub const CLEAR: i32 = 300;
/// An ordinary speaking voice.
pub const SPEAK: i32 = 500;

/// Loudness at `d` metres: an inverse-square-ish falloff, `vol · 100 / (100 + d²)`.
pub fn loudness(vol: i32, d: i32) -> i32 {
    let d = d.abs() as i64;
    ((vol as i64 * 100) / (100 + d * d)) as i32
}

/// The volume a speaker uses so a listener `d` metres away hears clearly, capped by their voice.
pub fn volume_for(d: i32, voice: i32) -> i32 {
    let d = d.abs() as i64;
    let need = (CLEAR as i64 + 60) * (100 + d * d) / 100;
    (need as i32).max(SPEAK).min(voice)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Heard { Missed, Clear(Utt), Misheard(Utt) }

/// What a listener makes of `u` at `loud`, given a roll in 0..1000.
pub fn hear(u: Utt, loud: i32, roll: i32) -> Heard {
    if loud < HEAR_MIN { return Heard::Missed; }
    if loud >= CLEAR { return Heard::Clear(u); }
    let chance = (CLEAR - loud) * 1000 / (CLEAR - HEAR_MIN);
    if roll < chance / 2 { Heard::Missed } else if roll < chance { Heard::Misheard(u.garbled()) } else { Heard::Clear(u) }
}
