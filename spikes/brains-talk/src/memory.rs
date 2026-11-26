//! Memory layers: short-term traces, long-term beliefs, emotional episodes, compiled habits,
//! and a relationship per other agent.

use crate::model::{Ent, Fact, Utt};

/// Where a belief came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    /// Known from the start (a founder's knowledge).
    Founding,
    Seen,
    /// Told by this agent, who was facing me.
    ToldBy(u8),
    /// Heard this agent say it to someone else.
    Overheard(u8),
    /// Worked out from other beliefs during retrospection.
    Derived,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Belief { pub fact: Fact, pub conf: i32, pub source: Source, pub tick: u32 }

/// Why a feeling arose; kept with the episode so it can be explained later.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cause { Greeting, Laugh, Snarl, Helped, Rebuffed, Betrayed, SharedLike }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TraceKind {
    /// `facing`: who the speaker was turned towards (a perceived cue, not an address).
    Heard { from: u8, facing: Option<u8>, utt: Utt, loud: i32 },
    Said { facing: Option<u8>, utt: Utt },
    Saw(Fact),
    Ate(Ent),
    Felt { with: Option<u8>, valence: i32, arousal: i32, cause: Cause },
}

/// A short-term memory item. `strength` falls each tick; at zero it's gone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Trace { pub tick: u32, pub strength: i32, pub kind: TraceKind }

impl Trace {
    pub fn arousal(&self) -> i32 {
        match self.kind {
            TraceKind::Felt { arousal, .. } => arousal,
            TraceKind::Heard { utt: Utt::Emote { arousal, .. }, .. } => arousal,
            _ => 0,
        }
    }
    pub fn involves(&self, who: u8) -> bool {
        match self.kind {
            TraceKind::Heard { from, .. } => from == who,
            TraceKind::Said { facing, .. } => facing == Some(who),
            TraceKind::Felt { with, .. } => with == Some(who),
            _ => false,
        }
    }
}

/// A feeling attached to a moment with someone. It fades slowly (per night), not per tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Episode { pub tick: u32, pub with: u8, pub valence: i32, pub arousal: i32, pub strength: i32, pub cause: Cause }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Need { Hunger }

/// A compiled, fast rule: "when this need is high, go here".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Habit { pub need: Need, pub place: Ent, pub strength: i32 }

/// What I make of someone. `knows` is theory of mind lite: facts I believe they hold.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Rel {
    pub other: u8,
    pub familiarity: i32,
    pub trust: i32,
    pub affinity: i32,
    pub meetings: u32,
    pub last_talk: u32,
    pub knows: Vec<Fact>,
    pub shared: Vec<Ent>,
}
