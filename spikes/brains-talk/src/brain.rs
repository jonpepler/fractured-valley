//! A brain: reads last tick's world (a `View`), writes only its own state and one action.

use crate::memory::*;
use crate::model::*;
use crate::world::{Body, View};
use crate::{clampv, pm, rng, sound};

/// Personality traits, per mille.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Personality {
    pub honesty: i32,
    pub warmth: i32,
    pub talk: i32,
    pub irritability: i32,
    pub trusting: i32,
    pub curiosity: i32,
}

impl Personality {
    /// Honest, friendly and chatty.
    pub const WARM: Personality = Personality { honesty: 900, warmth: 800, talk: 700, irritability: 200, trusting: 600, curiosity: 600 };
    /// Honest and curious, quieter, slower to trust.
    pub const SHY: Personality = Personality { honesty: 900, warmth: 600, talk: 400, irritability: 200, trusting: 400, curiosity: 800 };
    /// Looks after itself first; will lie when it pays.
    pub const SLY: Personality = Personality { honesty: 150, warmth: 300, talk: 500, irritability: 300, trusting: 300, curiosity: 300 };
    /// Decent, but short-tempered when hurting.
    pub const PRICKLY: Personality = Personality { honesty: 900, warmth: 600, talk: 500, irritability: 850, trusting: 500, curiosity: 500 };
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Mood { pub valence: i32, pub arousal: i32 }

/// The state of a conversation in progress, from this brain's side only.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Conv {
    pub with: u8,
    pub start: u32,
    pub my_turns: i32,
    pub their_turns: i32,
    pub last_heard: u32,
    pub i_greeted: bool,
    pub they_greeted: bool,
    pub i_bye: bool,
    pub they_bye: bool,
    pub bye_tick: u32,
    pub pending: Option<Utt>,
    pub asked_likes: bool,
    pub asked_feels: bool,
    pub asked_food: bool,
    pub last_laugh: u32,
    pub snarled: bool,
    /// They snarled at me during this conversation.
    pub stung: bool,
    /// Feeling recalled from emotional memory when the conversation began.
    pub recalled: i32,
    pub spoke_last: bool,
    pub they_spoke_last: bool,
}

/// What a brain asks the world to do next tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Idle,
    /// An utterance into the air. `facing` is who the speaker turns to; anyone in earshot hears.
    Say { utt: Utt, facing: Option<u8>, vol: i32 },
    Walk { to: Ent },
    Eat,
    Sleep,
    Wake,
}

pub const ST_CAPACITY: usize = 16;
pub const ST_DECAY: i32 = 8;
pub const LT_CAPACITY: usize = 48;
/// Talk to someone only this close (metres).
pub const TALK_RANGE: i32 = 4;
/// Ticks before starting another conversation with the same person.
pub const COOLDOWN: u32 = 700;
/// A belief this weak isn't acted on.
pub const ACT_MIN: i32 = 250;
pub const HUNGRY: i32 = 600;
/// Ticks between idle retrospection passes.
pub const RETRO_EVERY: u32 = 100;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Brain {
    pub id: u8,
    pub pers: Personality,
    pub mood: Mood,
    pub short: Vec<Trace>,
    pub long: Vec<Belief>,
    pub episodes: Vec<Episode>,
    pub rels: Vec<Rel>,
    pub habits: Vec<Habit>,
    pub conv: Option<Conv>,
    pub goal: Option<Ent>,
    /// Off only in control runs, to show that nothing survives without consolidation.
    pub retrospection: bool,
}

/// Explanation lines for the trace. Not part of brain state.
pub struct Notes { pub on: bool, pub lines: Vec<String> }
impl Notes {
    pub fn new(on: bool) -> Notes { Notes { on, lines: Vec::new() } }
    fn add(&mut self, f: impl FnOnce() -> String) { if self.on { self.lines.push(f()) } }
}

struct Cand { u: i32, act: Action, why: &'static str, reply: bool }

fn name(i: u8) -> String { Ent::Agent(i).word() }

/// A fact in the form it is held (so `is_in(food, the cold box)` stays distinct from `holds`).
pub fn fact_words(f: Fact) -> String {
    let p = match f.p { Pred::Likes => "likes", Pred::Fears => "fears", Pred::Holds => "holds", Pred::IsIn => "is_in", Pred::Feels => "feels" };
    format!("{}({}, {})", p, f.s.word(), f.o.word())
}

pub fn source_words(s: Source) -> String {
    match s {
        Source::Founding => "founding".into(),
        Source::Seen => "seen".into(),
        Source::ToldBy(x) => format!("told by {}", name(x)),
        Source::Overheard(x) => format!("overheard {}", name(x)),
        Source::Derived => "derived".into(),
    }
}

impl Brain {
    pub fn new(id: u8, pers: Personality) -> Brain {
        Brain { id, pers, mood: Mood { valence: pers.warmth / 4, arousal: 0 }, short: vec![], long: vec![], episodes: vec![], rels: vec![], habits: vec![], conv: None, goal: None, retrospection: true }
    }
    fn founding(mut self, fact: Fact) -> Brain {
        self.long.push(Belief { fact, conf: 1000, source: Source::Founding, tick: 0 });
        self
    }
    pub fn likes(self, e: Ent) -> Brain { let id = self.id; self.founding(Fact::new(Ent::Agent(id), Pred::Likes, e)) }
    pub fn fears(self, e: Ent) -> Brain { let id = self.id; self.founding(Fact::new(Ent::Agent(id), Pred::Fears, e)) }
    pub fn knows(self, f: Fact) -> Brain { self.founding(f) }

    pub fn rel(&mut self, j: u8) -> &mut Rel {
        if let Some(i) = self.rels.iter().position(|r| r.other == j) { return &mut self.rels[i]; }
        let trust = 300 + pm(self.pers.trusting, 400);
        self.rels.push(Rel { other: j, familiarity: 0, trust, affinity: 0, meetings: 0, last_talk: 0, knows: vec![], shared: vec![] });
        self.rels.sort_by_key(|r| r.other);
        let i = self.rels.iter().position(|r| r.other == j).unwrap();
        &mut self.rels[i]
    }
    pub fn rel_of(&self, j: u8) -> Option<&Rel> { self.rels.iter().find(|r| r.other == j) }
    pub fn belief(&self, f: Fact) -> Option<&Belief> { self.long.iter().find(|b| b.fact.same(f)) }

    fn push_short(&mut self, tr: Trace) {
        if self.short.len() >= ST_CAPACITY {
            let (i, _) = self.short.iter().enumerate().min_by_key(|(i, t)| (t.strength, *i)).unwrap();
            self.short.remove(i);
        }
        self.short.push(tr);
    }

    fn feel(&mut self, t: u32, with: Option<u8>, valence: i32, arousal: i32, cause: Cause) {
        self.push_short(Trace { tick: t, strength: 600 + arousal / 3, kind: TraceKind::Felt { with, valence, arousal, cause } });
        self.mood.valence = clampv(self.mood.valence + pm(valence, arousal) / 2);
        self.mood.arousal = self.mood.arousal.max(arousal);
    }

    // ---------------------------------------------------------------- the tick

    pub fn think(&mut self, v: &View, ex: &mut Notes) -> Action {
        let me = v.bodies[self.id as usize];
        let t = v.tick;
        if me.asleep {
            if !v.night { ex.add(|| "wakes up".into()); return Action::Wake; }
            return Action::Idle;
        }
        if v.night {
            if self.conv.is_some() { self.end_conv(t, "night falls", ex); }
            ex.add(|| "goes to sleep".into());
            self.retrospect(t, true, ex);
            return Action::Sleep;
        }
        self.decay(me);
        self.maybe_start_conv(v, me, ex);
        self.hear(v, me, ex);
        self.see(v, me, ex);
        self.maybe_end_conv(v, me, ex);
        // A periodic, staggered pass while idle, so things merely seen or overheard persist too.
        if self.conv.is_none() && !self.short.is_empty() && (t + self.id as u32 * 37) % RETRO_EVERY == 0 {
            self.retrospect(t, false, ex);
        }
        if self.conv.is_some() { self.choose_social(v, me, ex) } else { self.choose_body(v, me, ex) }
    }

    fn decay(&mut self, me: Body) {
        for tr in &mut self.short {
            let a = tr.arousal();
            tr.strength -= (ST_DECAY * (1000 - a / 2) / 1000).max(1);
        }
        self.short.retain(|t| t.strength > 0);
        let base = self.pers.warmth / 4 - me.pain / 2 - (me.hunger - 500).max(0) / 2;
        self.mood.valence += (base - self.mood.valence) / 12;
        self.mood.arousal -= self.mood.arousal / 12;
    }

    // ---------------------------------------------------------------- senses

    fn hear(&mut self, v: &View, me: Body, ex: &mut Notes) {
        if let Some(c) = &mut self.conv { c.they_spoke_last = false; }
        for s in v.heard {
            if s.from == self.id { continue; }
            let loud = sound::loudness(s.vol, s.pos - me.pos);
            let roll = rng::roll(v.seed, v.tick, self.id, 0x50 + s.from as u64);
            let u = match sound::hear(s.utt, loud, roll) {
                sound::Heard::Missed => {
                    if loud >= sound::HEAR_MIN / 2 { ex.add(|| format!("misses what {} said (loudness {})", name(s.from), loud)); }
                    continue;
                }
                sound::Heard::Misheard(u) => {
                    if u.render(s.from, s.facing) != s.utt.render(s.from, s.facing) { ex.add(|| format!("mishears {} (loudness {}): takes «{}» for «{}»", name(s.from), loud, s.utt.render(s.from, s.facing), u.render(s.from, s.facing))); }
                    u
                }
                sound::Heard::Clear(u) => u,
            };
            self.on_heard(s.from, s.facing, u, loud, v.tick, ex);
        }
    }

    fn on_heard(&mut self, from: u8, facing: Option<u8>, u: Utt, loud: i32, t: u32, ex: &mut Notes) {
        let salience = match u { Utt::Claim { .. } => 800, Utt::Ask { .. } => 500, Utt::Emote { arousal, .. } => 400 + arousal / 2, _ => 350 };
        self.push_short(Trace { tick: t, strength: salience, kind: TraceKind::Heard { from, facing, utt: u, loud } });
        let partner = self.conv.as_ref().map(|c| c.with) == Some(from);
        let to_me = facing == Some(self.id);
        if partner {
            let c = self.conv.as_mut().unwrap();
            c.last_heard = t;
            c.their_turns += 1;
            c.they_spoke_last = true;
        }
        match u {
            Utt::Greet { warmth } if partner => {
                self.conv.as_mut().unwrap().they_greeted = true;
                self.feel(t, Some(from), warmth / 2, 300, Cause::Greeting);
            }
            Utt::Emote { valence, arousal } if partner || to_me => {
                let cause = if valence >= 0 { Cause::Laugh } else { Cause::Snarl };
                if partner && valence < 0 { self.conv.as_mut().unwrap().stung = true; }
                self.feel(t, Some(from), valence, arousal, cause);
                ex.add(|| format!("feels {} (valence {}, arousal {})", if valence >= 0 { "warmed" } else { "stung" }, valence, arousal));
            }
            Utt::Claim { fact, conf } => {
                let trust = self.rel(from).trust;
                let c = pm(conf, trust);
                let how = if partner || to_me { "told" } else { "overheard" };
                ex.add(|| format!("{} {}: {} — holds it at conf {} (stated {} × trust {})", how, name(from), fact_words(fact), c, conf, trust));
                if let Some(seen) = self.long.iter().find(|b| b.source == Source::Seen && b.fact.contradicts(fact)).copied() {
                    let r = self.rel(from);
                    r.trust = pm(r.trust, 700);
                    let nt = r.trust;
                    ex.add(|| format!("that contradicts what I saw ({}): trust in {} → {}", fact_words(seen.fact), name(from), nt));
                }
                let asked = self.conv.as_ref().map(|c| c.asked_food).unwrap_or(false);
                if partner && asked && fact.canonical().p == Pred::Holds && fact.canonical().o == Ent::Food {
                    self.feel(t, Some(from), 300, 300, Cause::Helped);
                }
            }
            Utt::Ask { .. } if partner => self.conv.as_mut().unwrap().pending = Some(u),
            Utt::Farewell if partner => self.conv.as_mut().unwrap().they_bye = true,
            Utt::Refuse if partner => self.feel(t, Some(from), -250, 300, Cause::Rebuffed),
            _ => {}
        }
    }

    fn see(&mut self, v: &View, me: Body, ex: &mut Notes) {
        for p in v.places {
            if (p.pos - me.pos).abs() > 1 { continue; }
            let f = Fact::new(p.ent, Pred::Holds, p.holds);
            let seen_now = self.short.iter().any(|tr| tr.kind == TraceKind::Saw(f));
            let known = self.long.iter().any(|b| matches!(b.source, Source::Seen | Source::Founding) && b.fact.same(f));
            if seen_now || known { continue; }
            ex.add(|| format!("sees that {}", fact_words(f)));
            self.push_short(Trace { tick: v.tick, strength: 700, kind: TraceKind::Saw(f) });
            self.check_against_seen(f, v.tick, ex);
        }
    }

    /// A seen fact tests what others told me: liars lose trust, truth-tellers gain a little.
    fn check_against_seen(&mut self, f: Fact, t: u32, ex: &mut Notes) {
        let mut wrong: Vec<u8> = vec![];
        let mut right: Vec<u8> = vec![];
        let teller = |s: Source| match s { Source::ToldBy(x) | Source::Overheard(x) => Some(x), _ => None };
        for b in &self.long {
            if let Some(x) = teller(b.source) {
                if b.fact.contradicts(f) { wrong.push(x) } else if b.fact.same(f) { right.push(x) }
            }
        }
        self.long.retain(|b| !(b.fact.contradicts(f) && b.source != Source::Seen));
        for b in &mut self.long {
            if teller(b.source).is_some() && b.fact.same(f) { b.source = Source::Seen; b.conf = 1000; b.tick = t; }
        }
        for tr in &self.short {
            if let TraceKind::Heard { from, utt: Utt::Claim { fact, .. }, .. } = tr.kind {
                if fact.contradicts(f) { wrong.push(from) } else if fact.same(f) { right.push(from) }
            }
        }
        self.short.retain(|tr| !matches!(tr.kind, TraceKind::Heard { utt: Utt::Claim { fact, .. }, .. } if fact.contradicts(f)));
        if f.o != Ent::Food { self.habits.retain(|h| h.place != f.s); }
        wrong.sort(); wrong.dedup(); right.sort(); right.dedup();
        for x in wrong {
            let r = self.rel(x);
            let before = r.trust;
            r.trust = pm(r.trust, 400);
            r.affinity = clampv(r.affinity - 250);
            let after = r.trust;
            self.feel(t, Some(x), -600, 700, Cause::Betrayed);
            ex.add(|| format!("{} told me otherwise: trust in {} {} → {}, and I feel betrayed", name(x), name(x), before, after));
        }
        for x in right {
            let r = self.rel(x);
            r.trust += (1000 - r.trust) / 8;
            let after = r.trust;
            ex.add(|| format!("{} was right: trust in {} → {}", name(x), name(x), after));
        }
    }

    // ---------------------------------------------------------------- conversations

    fn maybe_start_conv(&mut self, v: &View, me: Body, ex: &mut Notes) {
        if self.conv.is_some() { return; }
        for (j, b) in v.bodies.iter().enumerate() {
            let j = j as u8;
            if j == self.id || b.asleep || (b.pos - me.pos).abs() > TALK_RANGE { continue; }
            // They greeted me just now, or they're free and we haven't talked lately.
            let greeted = v.heard.iter().any(|s| s.from == j && s.facing == Some(self.id) && matches!(s.utt, Utt::Greet { .. }));
            let busy = v.heard.iter().any(|s| s.from == j && s.facing.is_some() && s.facing != Some(self.id));
            let t = v.tick;
            let r = self.rel(j).clone();
            let cooling = r.meetings > 0 && t.saturating_sub(r.last_talk) < COOLDOWN;
            if !greeted && (busy || cooling) { continue; }
            let eps: Vec<Episode> = self.episodes.iter().filter(|e| e.with == j).copied().collect();
            let recalled = clampv(eps.iter().map(|e| pm(e.valence, e.strength)).sum());
            self.mood.valence = clampv(self.mood.valence + recalled / 2);
            ex.add(|| {
                let mut s = format!("starts a conversation with {} (familiarity {}, trust {}, affinity {})", name(j), r.familiarity, r.trust, r.affinity);
                if !eps.is_empty() {
                    let list: Vec<String> = eps.iter().map(|e| format!("{:?} on day {} (strength {})", e.cause, e.tick / crate::world::DAY + 1, e.strength)).collect();
                    s += &format!("; recalls {} → feeling {}", list.join(", "), recalled);
                }
                s
            });
            self.conv = Some(Conv { with: j, start: t, my_turns: 0, their_turns: 0, last_heard: t, i_greeted: false, they_greeted: false, i_bye: false, they_bye: false, bye_tick: 0, pending: None, asked_likes: false, asked_feels: false, asked_food: false, last_laugh: 0, snarled: false, stung: false, recalled, spoke_last: false, they_spoke_last: false });
            return;
        }
    }

    fn maybe_end_conv(&mut self, v: &View, me: Body, ex: &mut Notes) {
        let Some(c) = &self.conv else { return };
        let other = v.bodies[c.with as usize];
        let t = v.tick;
        let reason = if c.i_bye && c.they_bye {
            Some("we both said goodbye")
        } else if c.i_bye && t - c.bye_tick > 3 {
            Some("I said goodbye")
        } else if t - c.last_heard > 12 && t - c.start > 12 {
            Some("silence")
        } else if (other.pos - me.pos).abs() > TALK_RANGE * 3 || other.asleep {
            Some("out of earshot")
        } else {
            None
        };
        if let Some(r) = reason { self.end_conv(t, r, ex) }
    }

    fn end_conv(&mut self, t: u32, why: &str, ex: &mut Notes) {
        let c = self.conv.take().unwrap();
        let r = self.rel(c.with);
        r.last_talk = t;
        r.meetings += 1;
        ex.add(|| format!("conversation with {} ends ({}): I spoke {} times, they {}", name(c.with), why, c.my_turns, c.their_turns));
        self.retrospect(t, false, ex);
    }

    fn thinks_knows(&self, o: u8, f: Fact) -> bool {
        self.rel_of(o).map(|r| r.knows.iter().any(|k| k.same(f))).unwrap_or(false)
            || self.short.iter().any(|tr| match tr.kind {
                TraceKind::Said { facing, utt: Utt::Claim { fact, .. } } => facing == Some(o) && fact.same(f),
                TraceKind::Heard { from, utt: Utt::Claim { fact, .. }, .. } => from == o && fact.same(f),
                _ => false,
            })
    }

    fn knows_likes_of(&self, o: u8) -> bool {
        let is = |f: Fact| f.s == Ent::Agent(o) && f.p == Pred::Likes;
        self.long.iter().any(|b| is(b.fact))
            || self.short.iter().any(|tr| matches!(tr.kind, TraceKind::Heard { utt: Utt::Claim { fact, .. }, .. } if is(fact)))
    }

    fn my_feeling(&self, me: Body) -> Ent {
        if me.pain >= 400 { Ent::Hurt } else if self.mood.valence >= 0 { Ent::Glad } else { Ent::Low }
    }

    fn choose_social(&mut self, v: &View, me: Body, ex: &mut Notes) -> Action {
        // A question about my likes that I've already answered by volunteering needs no reply.
        if let Some(Utt::Ask { s: Some(Ent::Agent(x)), p: Pred::Likes, .. }) = self.conv.as_ref().unwrap().pending {
            let o = self.conv.as_ref().unwrap().with;
            let mine: Vec<Fact> = self.long.iter().filter(|b| b.fact.s == Ent::Agent(self.id) && b.fact.p == Pred::Likes).map(|b| b.fact).collect();
            if x == self.id && !mine.is_empty() && mine.iter().all(|f| self.thinks_knows(o, *f)) {
                self.conv.as_mut().unwrap().pending = None;
            }
        }
        let c = self.conv.clone().unwrap();
        let o = c.with;
        let t = v.tick;
        let r = self.rel(o).clone();
        let p = self.pers;
        let d = v.bodies[o as usize].pos - me.pos;
        let vol = sound::volume_for(d, me.voice);
        let say = |utt: Utt| Action::Say { utt, facing: Some(o), vol };
        let warm_now = clampv(r.affinity + c.recalled + self.mood.valence / 2);
        let mut cands: Vec<Cand> = vec![];
        let mut add = |u: i32, act: Action, why: &'static str, reply: bool| cands.push(Cand { u, act, why, reply });

        let quiet = if c.spoke_last { 650 } else if c.they_spoke_last { 50 } else { 250 };
        add(quiet, Action::Idle, "let them speak", false);

        if !c.i_greeted && !c.i_bye && !c.snarled {
            let w = clampv(pm(p.warmth, 400) + pm(warm_now, 600));
            add(900, say(Utt::Greet { warmth: w }), "greet", false);
        }

        if let Some(q) = c.pending {
            match q {
                Utt::Ask { s: None, p: Pred::Holds, o: Some(Ent::Food) } => match self.where_for_food() {
                    Some((place, conf, _)) => {
                        add(500 + pm(p.honesty, 300) + pm(p.warmth, 200) + r.affinity / 5,
                            say(Utt::Claim { fact: Fact::new(place, Pred::Holds, Ent::Food), conf }), "answer truthfully", true);
                        if let Some(decoy) = v.places.iter().map(|pl| pl.ent).find(|e| *e != place) {
                            add(300 + pm(1000 - p.honesty, me.hunger) - r.affinity / 2,
                                say(Utt::Claim { fact: Fact::new(decoy, Pred::Holds, Ent::Food), conf: 900 }), "send them away from my food (a lie)", true);
                        }
                    }
                    None => add(600, say(Utt::DontKnow), "say I don't know", true),
                },
                Utt::Ask { s: Some(Ent::Agent(x)), p: Pred::Likes, .. } if x == self.id => {
                    let like = self.long.iter().find(|b| b.fact.s == Ent::Agent(self.id) && b.fact.p == Pred::Likes && !self.thinks_knows(o, b.fact)).map(|b| b.fact);
                    match like {
                        Some(f) => add(600 + p.talk / 5 + warm_now / 5, say(Utt::Claim { fact: f, conf: 900 }), "answer what I like", true),
                        None => add(400, say(Utt::DontKnow), "nothing new to say", true),
                    }
                }
                Utt::Ask { s: Some(Ent::Agent(x)), p: Pred::Feels, .. } if x == self.id => {
                    let f = Fact::new(Ent::Agent(self.id), Pred::Feels, self.my_feeling(me));
                    add(700, say(Utt::Claim { fact: f, conf: 900 }), "say how I am", true);
                }
                _ => add(300, say(Utt::DontKnow), "say I don't know", true),
            }
            add(200 - warm_now / 2 + pm(p.irritability, me.pain) / 2, say(Utt::Refuse), "refuse to answer", true);
        }

        if c.i_greeted && !c.i_bye {
            for b in self.long.clone() {
                let f = b.fact.canonical();
                if self.thinks_knows(o, f) { continue; }
                if f.s == Ent::Agent(self.id) && f.p == Pred::Likes {
                    add(350 + pm(p.talk, 300) + warm_now / 5, say(Utt::Claim { fact: f, conf: 900 }), "tell them about myself", false);
                } else if f.s == Ent::Agent(self.id) && f.p == Pred::Fears {
                    add(200 + pm(p.talk, 200) + (r.trust - 500) / 2 + warm_now / 5, say(Utt::Claim { fact: f, conf: 900 }), "confide a fear", false);
                } else if !matches!(f.s, Ent::Agent(_)) && b.conf >= 600 {
                    let mut u = 150 + pm(p.warmth, 200) + pm(p.talk, 150) + warm_now / 10;
                    if f.o == Ent::Food { u -= pm(1000 - p.honesty, me.hunger) }
                    add(u, say(Utt::Claim { fact: f, conf: b.conf }), "share what I know", false);
                }
            }
            if !c.asked_likes && !self.knows_likes_of(o) {
                add(300 + pm(p.curiosity, pm(400, 1000 - r.familiarity)) + warm_now / 10,
                    say(Utt::Ask { s: Some(Ent::Agent(o)), p: Pred::Likes, o: None }), "get to know them", false);
            }
            if !c.asked_feels && r.familiarity >= 300 {
                add(350 + r.affinity / 3, say(Utt::Ask { s: Some(Ent::Agent(o)), p: Pred::Feels, o: None }), "ask how they are", false);
            }
            if !c.asked_food && me.hunger >= 500 && self.where_for_food().is_none() {
                add(400 + me.hunger / 2, say(Utt::Ask { s: None, p: Pred::Holds, o: Some(Ent::Food) }), "I'm hungry and don't know where food is", false);
            }
            if t - c.last_laugh > 4 && self.mood.valence > 0 && warm_now > 0 {
                add(150 + self.mood.valence / 3 + r.affinity / 3 + pm(p.talk, 150), say(Utt::Emote { valence: 600, arousal: 600 }), "share a laugh", false);
            }
        }
        let foul = me.pain + (-self.mood.valence).max(0);
        if !c.snarled && !c.i_bye && foul > 0 {
            add(100 + pm(p.irritability, foul), Action::Say { utt: Utt::Emote { valence: -800, arousal: 900 }, facing: Some(o), vol: me.voice }, "I'm in a foul mood", false);
        }
        if !c.i_bye && (c.i_greeted || c.they_bye || c.snarled || c.stung) {
            let mut u = 100 + 90 * c.my_turns - r.affinity / 4 - pm(p.talk, 200) + (-warm_now).max(0) / 2 + (-self.mood.valence).max(0) / 2;
            if c.they_bye { u += 900 }
            if c.stung { u += 600 }
            if me.hunger >= HUNGRY && self.where_for_food().is_some() { u += 500 }
            add(u, say(Utt::Farewell), "leave", false);
        }

        for (k, cd) in cands.iter_mut().enumerate() {
            cd.u += rng::roll(v.seed, t, self.id, 0x100 + k as u64) % 30;
        }
        let mut order: Vec<usize> = (0..cands.len()).collect();
        order.sort_by_key(|&k| (-cands[k].u, k));
        let best = &cands[order[0]];
        if ex.on && !matches!(best.act, Action::Idle) {
            let alts: Vec<String> = order.iter().skip(1).take(2).map(|&k| format!("{} {}", cands[k].why, cands[k].u)).collect();
            ex.lines.push(format!("chooses to {} ({}) over: {}", best.why, best.u, alts.join(", ")));
        }
        let act = best.act;
        let reply = best.reply;
        let c = self.conv.as_mut().unwrap();
        c.spoke_last = matches!(act, Action::Say { .. });
        if let Action::Say { utt, facing, .. } = act {
            c.my_turns += 1;
            if reply { c.pending = None }
            match utt {
                Utt::Greet { .. } => c.i_greeted = true,
                Utt::Farewell => { c.i_bye = true; c.bye_tick = t }
                Utt::Emote { valence, .. } if valence >= 0 => c.last_laugh = t,
                Utt::Emote { .. } => c.snarled = true,
                Utt::Ask { p: Pred::Likes, .. } => c.asked_likes = true,
                Utt::Ask { p: Pred::Feels, .. } => c.asked_feels = true,
                Utt::Ask { p: Pred::Holds, .. } => c.asked_food = true,
                _ => {}
            }
            self.push_short(Trace { tick: t, strength: 400, kind: TraceKind::Said { facing, utt } });
        }
        act
    }

    // ---------------------------------------------------------------- body needs

    /// Where to go for food, and which memory layer answered: the compiled habit first, then
    /// long-term beliefs (in any form), then things just heard.
    pub fn where_for_food(&self) -> Option<(Ent, i32, &'static str)> {
        let seen_not = |pl: Ent| {
            self.short.iter().any(|tr| matches!(tr.kind, TraceKind::Saw(f) if f.s == pl && f.o != Ent::Food))
                || self.long.iter().any(|b| b.source == Source::Seen && b.fact.s == pl && b.fact.p == Pred::Holds && b.fact.o != Ent::Food)
        };
        if let Some(h) = self.habits.iter().filter(|h| h.need == Need::Hunger && h.strength >= 300 && !seen_not(h.place)).max_by_key(|h| (h.strength, std::cmp::Reverse(h.place))) {
            return Some((h.place, h.strength, "habit"));
        }
        let food = |f: Fact| { let f = f.canonical(); f.p == Pred::Holds && f.o == Ent::Food };
        if let Some(b) = self.long.iter().filter(|b| food(b.fact) && b.conf >= ACT_MIN && !seen_not(b.fact.canonical().s)).max_by_key(|b| (b.conf, std::cmp::Reverse(b.fact.canonical().s))) {
            return Some((b.fact.canonical().s, b.conf, if b.fact.p == Pred::IsIn { "long-term belief, 'food is in X' form" } else { "long-term belief, 'X holds food' form" }));
        }
        let mut best: Option<(Ent, i32, &'static str)> = None;
        for tr in &self.short {
            if let TraceKind::Heard { from, utt: Utt::Claim { fact, conf }, .. } = tr.kind {
                let trust = self.rel_of(from).map(|r| r.trust).unwrap_or(500);
                let c = pm(conf, trust);
                if food(fact) && c >= ACT_MIN && !seen_not(fact.canonical().s) && best.map(|b| c > b.1).unwrap_or(true) {
                    best = Some((fact.canonical().s, c, "just heard (short-term)"));
                }
            }
        }
        best
    }

    fn choose_body(&mut self, v: &View, me: Body, ex: &mut Notes) -> Action {
        if me.hunger < HUNGRY { self.goal = None; return Action::Idle; }
        if let Some(pl) = v.places.iter().find(|pl| (pl.pos - me.pos).abs() <= 1 && pl.holds == Ent::Food) {
            let at = pl.ent;
            ex.add(|| format!("hungry ({}) and food is here: eats", me.hunger));
            self.push_short(Trace { tick: v.tick, strength: 700, kind: TraceKind::Ate(at) });
            self.goal = None;
            return Action::Eat;
        }
        match self.where_for_food() {
            Some((pl, conf, path)) => {
                if self.goal != Some(pl) {
                    ex.add(|| format!("hungry ({}): heads for {} — from {}, conf {}", me.hunger, pl.word(), path, conf));
                    self.goal = Some(pl);
                }
                Action::Walk { to: pl }
            }
            None => {
                if self.goal != Some(Ent::Nothing) {
                    let doubted: Vec<String> = self.long.iter().filter(|b| { let f = b.fact.canonical(); f.p == Pred::Holds && f.o == Ent::Food && b.conf < ACT_MIN })
                        .map(|b| format!("{} ({}, conf {} too low)", fact_words(b.fact), source_words(b.source), b.conf)).collect();
                    ex.add(|| if doubted.is_empty() { format!("hungry ({}) but knows nowhere to find food", me.hunger) } else { format!("hungry ({}) but doesn't believe {}", me.hunger, doubted.join(", ")) });
                    self.goal = Some(Ent::Nothing);
                }
                Action::Idle
            }
        }
    }

    // ---------------------------------------------------------------- retrospection

    fn learn(&mut self, fact: Fact, conf: i32, source: Source, t: u32, out: &mut Vec<String>) {
        if let Some(i) = self.long.iter().position(|b| b.fact.same(fact)) {
            let b = &mut self.long[i];
            if conf > b.conf { b.conf = conf; b.source = source; b.tick = t; }
            else if b.source != source { b.conf += (1000 - b.conf) / 10; }
            return;
        }
        if let Some(i) = self.long.iter().position(|b| b.fact.contradicts(fact)) {
            let old = self.long[i];
            if old.source == Source::Seen && source != Source::Seen { out.push(format!("ignores {} ({}): I saw otherwise", fact_words(fact), source_words(source))); return; }
            if conf < old.conf { out.push(format!("keeps {} over {}", fact_words(old.fact), fact_words(fact))); return; }
            self.long.remove(i);
        }
        if self.long.len() >= LT_CAPACITY {
            let (i, _) = self.long.iter().enumerate().min_by_key(|(i, b)| (b.conf, *i)).unwrap();
            self.long.remove(i);
        }
        self.long.push(Belief { fact, conf, source, tick: t });
        out.push(format!("+ belief {} ({}, conf {})", fact_words(fact), source_words(source), conf));
    }

    /// Consolidate short-term traces into beliefs, episodes, habits and relationships. After a
    /// conversation it consolidates; on sleep it also compiles habits and lets feelings fade.
    pub fn retrospect(&mut self, t: u32, sleep: bool, ex: &mut Notes) {
        if !self.retrospection { return; }
        let traces = std::mem::take(&mut self.short);
        let mut out: Vec<String> = vec![];
        let mut partners: Vec<u8> = vec![];
        for tr in &traces {
            match tr.kind {
                TraceKind::Heard { from, facing, utt, .. } => {
                    let addressed = facing == Some(self.id);
                    if addressed { partners.push(from) }
                    if let Utt::Claim { fact, conf } = utt {
                        let trust = self.rel(from).trust;
                        let src = if addressed { Source::ToldBy(from) } else { Source::Overheard(from) };
                        self.learn(fact, pm(conf, trust), src, t, &mut out);
                        let r = self.rel(from);
                        if !r.knows.iter().any(|k| k.same(fact)) { r.knows.push(fact) }
                        if fact.s == Ent::Agent(from) && fact.p == Pred::Likes {
                            let mine = Fact::new(Ent::Agent(self.id), Pred::Likes, fact.o);
                            if self.belief(mine).is_some() && !self.rel(from).shared.contains(&fact.o) {
                                let r = self.rel(from);
                                r.shared.push(fact.o);
                                r.affinity = clampv(r.affinity + 120);
                                self.episodes.push(Episode { tick: tr.tick, with: from, valence: 400, arousal: 400, strength: 400, cause: Cause::SharedLike });
                                out.push(format!("we both like {}: affinity with {} +120", fact.o.word(), name(from)));
                            }
                            // Derive: they like what a place holds, so they'd like the place.
                            let places: Vec<Ent> = self.long.iter().filter(|b| { let c = b.fact.canonical(); c.p == Pred::Holds && c.o == fact.o }).map(|b| b.fact.canonical().s).collect();
                            for pl in places {
                                self.learn(Fact::new(Ent::Agent(from), Pred::Likes, pl), pm(pm(conf, trust), 700), Source::Derived, t, &mut out);
                            }
                        }
                    }
                }
                TraceKind::Said { facing: Some(to), utt } => {
                    partners.push(to);
                    if let Utt::Claim { fact, .. } = utt {
                        let r = self.rel(to);
                        if !r.knows.iter().any(|k| k.same(fact)) { r.knows.push(fact) }
                    }
                }
                TraceKind::Said { .. } => {}
                TraceKind::Saw(f) => self.learn(f, 1000, Source::Seen, t, &mut out),
                TraceKind::Ate(pl) => {
                    match self.habits.iter_mut().find(|h| h.need == Need::Hunger && h.place == pl) {
                        Some(h) => h.strength = (h.strength + 150).min(1000),
                        None => self.habits.push(Habit { need: Need::Hunger, place: pl, strength: 400 }),
                    }
                    out.push(format!("habit: hungry → {} (strengthened by eating there)", pl.word()));
                }
                TraceKind::Felt { with: Some(x), valence, arousal, cause } => {
                    let r = self.rel(x);
                    r.affinity = clampv(r.affinity + valence * arousal / 4000);
                    let intensity = valence.abs() * arousal / 1000;
                    if intensity >= 300 {
                        self.episodes.push(Episode { tick: tr.tick, with: x, valence, arousal, strength: intensity, cause });
                        out.push(format!("+ episode {:?} with {} (valence {}, strength {})", cause, name(x), valence, intensity));
                    }
                }
                TraceKind::Felt { .. } => {}
            }
        }
        partners.sort();
        partners.dedup();
        for x in partners {
            let r = self.rel(x);
            r.familiarity += (1000 - r.familiarity) / 5;
            let (f, a, tr) = (r.familiarity, r.affinity, r.trust);
            out.push(format!("{}: familiarity {}, affinity {}, trust {}", name(x), f, a, tr));
        }
        if sleep {
            let food: Vec<(Ent, i32)> = self.long.iter().filter(|b| { let c = b.fact.canonical(); c.p == Pred::Holds && c.o == Ent::Food && b.conf >= 500 }).map(|b| (b.fact.canonical().s, b.conf)).collect();
            for (pl, conf) in food {
                let s = pm(conf, 800);
                match self.habits.iter_mut().find(|h| h.need == Need::Hunger && h.place == pl) {
                    Some(h) if h.strength >= s => {}
                    Some(h) => h.strength = s,
                    None => { self.habits.push(Habit { need: Need::Hunger, place: pl, strength: s }); out.push(format!("compiles: hungry → {} (strength {})", pl.word(), s)); }
                }
            }
            for e in &mut self.episodes { e.strength -= e.strength / 8; }
            self.episodes.retain(|e| e.strength >= 60);
        }
        let n = traces.len();
        if out.is_empty() && !sleep { return; }
        ex.add(|| format!("retrospection{} over {} short-term traces{}{}", if sleep { " (sleep)" } else { "" }, n, if out.is_empty() { "" } else { ": " }, out.join("; ")));
    }

    /// A readable snapshot of this brain's memory and relationships.
    pub fn describe(&self) -> String {
        let mut s = format!("  {} — mood {}, short-term {} items\n", name(self.id), self.mood.valence, self.short.len());
        for b in &self.long { s += &format!("    belief  {:<34} {:<16} conf {}\n", fact_words(b.fact), source_words(b.source), b.conf); }
        for h in &self.habits { s += &format!("    habit   hungry → {} (strength {})\n", h.place.word(), h.strength); }
        for e in &self.episodes { s += &format!("    episode {:?} with {}, day {} (valence {}, strength {})\n", e.cause, name(e.with), e.tick / crate::world::DAY + 1, e.valence, e.strength); }
        for r in &self.rels { s += &format!("    rel     {:<4} familiarity {:>3}, trust {:>3}, affinity {:>4}, meetings {}, thinks they know {} facts\n", name(r.other), r.familiarity, r.trust, r.affinity, r.meetings, r.knows.len()); }
        s
    }
}
