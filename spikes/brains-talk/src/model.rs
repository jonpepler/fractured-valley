//! What can be thought about and said. Utterances carry structured content, never text; English
//! appears only when the trace renders them.

/// Things a brain can hold beliefs about. Places, substances, feelings and other agents.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Ent { Agent(u8), ColdBox, Well, Meadow, Food, Water, Flowers, Dog, Nothing, Glad, Low, Hurt }

pub const NAMES: [&str; 4] = ["Ada", "Bo", "Cal", "Dee"];

impl Ent {
    pub fn word(self) -> String {
        use Ent::*;
        match self {
            Agent(i) => NAMES[i as usize % NAMES.len()].to_string(),
            ColdBox => "the cold box".into(),
            Well => "the well".into(),
            Meadow => "the meadow".into(),
            Food => "food".into(),
            Water => "water".into(),
            Flowers => "flowers".into(),
            Dog => "the dog".into(),
            Nothing => "nothing".into(),
            Glad => "glad".into(),
            Low => "low".into(),
            Hurt => "hurt".into(),
        }
    }
    /// What a garbled word becomes when misheard.
    pub fn sounds_like(self) -> Ent {
        use Ent::*;
        match self {
            Food => Flowers, Flowers => Food, Well => Meadow, Meadow => Well, ColdBox => Well,
            Water => Nothing, Glad => Low, Low => Glad, e => e,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Pred { Likes, Fears, Holds, IsIn, Feels }

/// A subject–predicate–object triple. `Holds(place, thing)` and `IsIn(thing, place)` are two
/// forms of the same fact; `canonical` maps both to the `Holds` form, which is the one spoken.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Fact { pub s: Ent, pub p: Pred, pub o: Ent }

impl Fact {
    pub const fn new(s: Ent, p: Pred, o: Ent) -> Fact { Fact { s, p, o } }
    pub fn canonical(self) -> Fact {
        if self.p == Pred::IsIn { Fact::new(self.o, Pred::Holds, self.s) } else { self }
    }
    pub fn single_valued(self) -> bool { matches!(self.canonical().p, Pred::Holds | Pred::Feels) }
    pub fn same(self, other: Fact) -> bool { self.canonical() == other.canonical() }
    /// Same subject and single-valued predicate, different object.
    pub fn contradicts(self, other: Fact) -> bool {
        let (a, b) = (self.canonical(), other.canonical());
        a.single_valued() && a.s == b.s && a.p == b.p && a.o != b.o
    }
}

/// The content of an in-world speech act.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Utt {
    /// `warmth` -1000..1000.
    Greet { warmth: i32 },
    /// A claim about the world or oneself, with the speaker's stated confidence.
    Claim { fact: Fact, conf: i32 },
    /// A question as a pattern: `None` is the unknown slot.
    Ask { s: Option<Ent>, p: Pred, o: Option<Ent> },
    DontKnow,
    Refuse,
    /// An emotional signal (laugh, snarl), `valence` -1000..1000, `arousal` 0..1000.
    Emote { valence: i32, arousal: i32 },
    Farewell,
}

impl Utt {
    pub fn garbled(self) -> Utt {
        match self {
            Utt::Claim { fact, conf } => Utt::Claim { fact: Fact::new(fact.s, fact.p, fact.o.sounds_like()), conf },
            Utt::Ask { s, p, o } => Utt::Ask { s, p, o: o.map(Ent::sounds_like) },
            Utt::Greet { warmth } => Utt::Greet { warmth: warmth / 2 },
            u => u,
        }
    }

    /// English for the trace only. `speaker` is "I"; `listener` (the one faced) is "you".
    pub fn render(&self, speaker: u8, listener: Option<u8>) -> String {
        let who = |e: Ent| -> String {
            if e == Ent::Agent(speaker) { "I".into() } else if Some(e) == listener.map(Ent::Agent) { "you".into() } else { e.word() }
        };
        let cap = |s: String| { let mut c = s.chars(); match c.next() { Some(f) => f.to_uppercase().collect::<String>() + c.as_str(), None => s } };
        match *self {
            Utt::Greet { warmth } if warmth >= 500 => "Hello, friend! Good to see you.".into(),
            Utt::Greet { warmth } if warmth >= 150 => "Hello.".into(),
            Utt::Greet { .. } => "...Hi.".into(),
            Utt::Claim { fact, conf } => {
                let f = fact.canonical();
                let me = f.s == Ent::Agent(speaker);
                let you = Some(f.s) == listener.map(Ent::Agent);
                let body = match f.p {
                    Pred::Likes => format!("{} {} {}", who(f.s), if me || you { "like" } else { "likes" }, f.o.word()),
                    Pred::Fears => format!("{}{} afraid of {}", who(f.s), if me { "'m" } else if you { " are" } else { " is" }, f.o.word()),
                    Pred::Feels => format!("{}{} {}", who(f.s), if me { "'m" } else if you { " are" } else { " is" }, f.o.word()),
                    _ => format!("{} holds {}", who(f.s), f.o.word()),
                };
                if conf < 500 { format!("I think {}.", body) } else { format!("{}.", cap(body)) }
            }
            Utt::Ask { s: None, p: Pred::Holds, o: Some(o) } => format!("Where can I find {}?", o.word()),
            Utt::Ask { s: Some(_), p: Pred::Likes, o: None } => "What do you like?".into(),
            Utt::Ask { s: Some(_), p: Pred::Feels, o: None } => "How are you?".into(),
            Utt::Ask { .. } => "Hm?".into(),
            Utt::DontKnow => "I don't know.".into(),
            Utt::Refuse => "I'd rather not say.".into(),
            Utt::Emote { valence, arousal } if valence >= 0 && arousal >= 500 => "(laughs)".into(),
            Utt::Emote { valence, .. } if valence >= 0 => "(smiles)".into(),
            Utt::Emote { arousal, .. } if arousal >= 500 => "(snarls) Leave me alone!".into(),
            Utt::Emote { .. } => "(sighs)".into(),
            Utt::Farewell => "Goodbye.".into(),
        }
    }
}
