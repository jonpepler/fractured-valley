//! The foundational set, written in the language (text, parsed at start-up), plus the schedule
//! alternatives and the tool-maker itself. Every one of these is hand-written; the tool-maker's
//! output is not.
//!
//! Conventions a mind's tool layer (`belt`) fills in: `day` and `tod` (12 buckets a day) come
//! from the tick; `who` is an entity the mind tracks; `place` is the mind's own place index.

use super::lang::{parse, Desc};

/// Where is X at this time of day: the 3 places most seen per (who, time of day). Fades by 1/8 a night.
pub const TIMETABLE: &str = r#"
(tool "timetable"
  (slot when (key entity tod) (votes 3) place 512)
  (observe (if (= kind seen) (put when (who tod) place 1)))
  (ask (top when (who tod) 3))
  (forget (fade when 3)))"#;

/// Last seen, and if that was long ago, the places X is most often at (a habit counter).
pub const LAST_AND_HABIT: &str = r#"
(tool "last-and-habit"
  (slot last (key entity) last place 256)
  (slot habit (key entity) (votes 4) place 256)
  (observe (if (= kind seen) (put last (who) place 1) (put habit (who) place 1)))
  (ask (if (< (- tick (get last (who) at)) 30) (top last (who) 1) (top habit (who) 3)))
  (forget (fade habit 3)))"#;

/// A sequence: where X goes after each place, and how long X stays; asked, start where X was
/// last seen and step forward one place per average stay since X arrived there.
pub const SEQUENCE: &str = r#"
(tool "sequence"
  (slot last (key entity) last place 256)
  (slot arrived (key entity) last tick 256)
  (slot next (key entity place) (votes 3) place 512)
  (slot stay (key entity) count int 256)
  (observe
    (if (and (= kind seen) (!= place (get last (who) top0)))
      (put next (who (get last (who) top0)) place 1)
      (put stay (who) 0 (- tick (get arrived (who) top0)))
      (put arrived (who) tick 1))
    (if (= kind seen) (put last (who) place 1)))
  (ask (chain next (who it) (get last (who) top0)
    (/ (- tick (get arrived (who) top0)) (max 1 (/ (get stay (who) count) (max 1 (get stay (who) w0)))))))
  (forget (fade next 3)))"#;

/// The timetable with context and back-off: (who, time of day, where last seen), else (who, time of
/// day), else (who).
pub const CONTEXT_TIMETABLE: &str = r#"
(tool "context-timetable"
  (slot last (key entity) last place 256)
  (slot ctx (key entity tod place) (votes 2) place 1024)
  (slot when (key entity tod) (votes 3) place 512)
  (slot any (key entity) (votes 3) place 256)
  (observe (if (= kind seen)
    (put ctx (who tod (get last (who) top0)) place 1)
    (put when (who tod) place 1)
    (put any (who) place 1)
    (put last (who) place 1)))
  (ask (first
    (top ctx (who tod (get last (who) top0)) 3)
    (top when (who tod) 3)
    (top any (who) 3)))
  (forget (fade ctx 3) (fade when 3) (fade any 3)))"#;

/// A timetable that forgets fast (half a night): keeps up with a routine that changes.
pub const FICKLE_TIMETABLE: &str = r#"
(tool "fickle-timetable"
  (slot when (key entity tod) (votes 3) place 512)
  (observe (if (= kind seen) (put when (who tod) place 2)))
  (ask (top when (who tod) 3))
  (forget (fade when 1)))"#;

/// A blend: the timetable's vote and the last sighting's, the last weighted by how recent it is.
pub const BLEND: &str = r#"
(tool "blend"
  (slot last (key entity) last place 256)
  (slot when (key entity tod) (votes 3) place 512)
  (observe (if (= kind seen) (put when (who tod) place 1) (put last (who) place 1)))
  (ask (if (< (- tick (get last (who) at)) 15)
     (blend (700 (top last (who) 1)) (300 (top when (who tod) 3)))
     (blend (200 (top last (who) 1)) (800 (top when (who tod) 3)))))
  (forget (fade when 3)))"#;

/// The schedule alternatives, for comparison.
pub const SCHEDULES: [&str; 6] = [TIMETABLE, LAST_AND_HABIT, SEQUENCE, CONTEXT_TIMETABLE, FICKLE_TIMETABLE, BLEND];

/// Route memory: on the way to a goal, which place I went to next from here (a blocked step is dropped).
pub const ROUTES: &str = r#"
(tool "routes"
  (slot next (key place place) (votes 2) place 512)
  (observe
    (if (and (= kind moved) (or (= val none) (<= 0 val))) (put next (goal place) next 1))
    (if (and (= kind moved) (< val 0)) (drop next (goal place) next)))
  (ask (top next (goal place) 2))
  (forget (fade next 4)))"#;

/// Route memory followed to the end: from here towards a goal, up to 16 hops, where I end up
/// (it says whether my remembered route reaches the goal).
pub const ROUTE_END: &str = r#"
(tool "route-end"
  (slot next (key place place) (votes 2) place 512)
  (observe (if (= kind moved) (put next (goal place) next 1)))
  (ask (chain next (goal it) place 16)))"#;

/// What worked here: per place, the actions with the best summed outcome (signed weights).
pub const WHAT_WORKED: &str = r#"
(tool "what-worked"
  (slot worked (key place) (votes 4) act 256)
  (observe (if (= kind did) (put worked (place) act val)))
  (ask (top worked (place) 3))
  (forget (fade worked 4)))"#;

/// Who knows what (9.7): asked with `who`, does who know `what`; without, who knows `what`.
pub const WHO_KNOWS: &str = r#"
(tool "who-knows"
  (slot knows (key entity node) last int 1024)
  (slot knowers (key node) (votes 4) entity 256)
  (observe (if (= kind told)
    (put knows (to what) 1 1) (put knows (who what) 1 1)
    (put knowers (what) to 1) (put knowers (what) who 1)))
  (ask (if (!= who none) (val (get knows (who what) has)) (top knowers (what) 4))))"#;

/// Volatility: a fading total of surprise per thing.
pub const VOLATILITY: &str = r#"
(tool "volatility"
  (slot surprise (key node) count int 512)
  (observe (if (= kind surprise) (put surprise (what) 0 val)))
  (ask (val (get surprise (what) count)))
  (forget (fade surprise 2)))"#;

/// When is X around: a histogram of the (two-bucket) times of day X is seen.
pub const RHYTHM: &str = r#"
(tool "rhythm"
  (slot around (key entity) (hist 6) tod 256)
  (observe (if (= kind seen) (put around (who) (/ tod 2) 1)))
  (ask (top around (who) 2))
  (forget (fade around 4)))"#;

/// Who I meet at a place.
pub const WHO_IS_THERE: &str = r#"
(tool "who-is-there"
  (slot meet (key place) (votes 4) entity 256)
  (observe (if (= kind seen) (put meet (place) who 1)))
  (ask (top meet (place) 4))
  (forget (fade meet 3)))"#;

/// Trouble spots: surprise on an 8-cell grid.
pub const TROUBLE_SPOTS: &str = r#"
(tool "trouble-spots"
  (slot grid (key coord coord) count int 256)
  (observe (if (= kind surprise) (put grid ((/ x 8) (/ y 8)) 0 val)))
  (ask (val (get grid ((/ x 8) (/ y 8)) count)))
  (forget (fade grid 2)))"#;

/// A favour ledger: what others did for me (val > 0) or against me, per person.
pub const LEDGER: &str = r#"
(tool "ledger"
  (slot owed (key entity) count int 128)
  (observe (if (and (= kind did) (!= who none)) (put owed (who) 0 val)))
  (ask (val (get owed (who) count)))
  (forget (fade owed 5)))"#;

/// Composition (round-2 revision): how do I get to where X is now? Asks the timetable where X
/// is at this time of day, then the route memory for the next step there. Both sub-tools' costs
/// come out of this one call's fuel, and its static bound includes theirs.
pub const GO_FIND: &str = r#"
(tool "go-find"
  (uses timetable routes)
  (ask (sub routes "" ((place place) (goal (sub timetable "" ((who who) (tod tod)) 0))))))"#;

/// Play (see the note's catalogue): small tools written to see how easy the language is.
/// When does trouble happen: surprise per time of day.
pub const DANGER_CLOCK: &str = r#"
(tool "danger-clock"
  (slot when (key tod) count int 12)
  (observe (if (= kind surprise) (put when (tod) 0 val)))
  (ask (val (get when (tod) count)))
  (forget (fade when 2)))"#;

/// Who is usually at a place at this time of day (the timetable turned round).
pub const WHO_WILL_BE_THERE: &str = r#"
(tool "who-will-be-there"
  (slot there (key place tod) (votes 4) entity 512)
  (observe (if (= kind seen) (put there (place tod) who 1)))
  (ask (top there (place tod) 4))
  (forget (fade there 3)))"#;

/// Composition: would telling `to` about `what` be news? Asks who-knows.
pub const IS_IT_NEWS: &str = r#"
(tool "is-it-news"
  (uses who-knows)
  (ask (val (if (= (sub who-knows "" ((who to) (what what)) 0) 1) 0 1))))"#;

/// Composition on a made map: the places next to my goal (where to wait, or to look around),
/// through the map's "near" view. Works with whatever map this mind made, or none.
pub const NEAR_GOAL: &str = r#"
(tool "near-goal"
  (uses map)
  (ask (sub map "near" ((place goal)))))"#;

pub const PLAY: [&str; 4] = [DANGER_CLOCK, WHO_WILL_BE_THERE, IS_IT_NEWS, NEAR_GOAL];

pub const FOUNDATION: [&str; 11] = [TIMETABLE, ROUTES, ROUTE_END, WHAT_WORKED, WHO_KNOWS, VOLATILITY, RHYTHM, WHO_IS_THERE, TROUBLE_SPOTS, LEDGER, LAST_AND_HABIT];

/// The tool-maker, itself a tool. Its questions slot counts the cost of what was thought
/// through, per question (the field wanted, the kind of record that answers it, the fields
/// given, the subject); its log keeps the last 4096 things seen, done and walked; its library
/// the shapes that won. The grammar is the default maker's character.
pub const MAKER: &str = r#"
(tool "tool-maker"
  (slot questions (key field kind int entity) count int 64)
  (slot log (key) (ring 4096) int 1)
  (slot lib (key) (votes 4) int 1)
  (observe
    (if (= kind asked) (put questions (want from given who) 0 cost))
    (if (!= kind asked) (push log)))
  (ask (make questions log lib
    (grammar (feats who tod (/ tod 4) (% day 2) (% day 3) prev (since 25) place goal)
      (cells (votes 3) last) (keys 3) backoff paths (cap 512) (per-kb 20) (per-fuel 2) (budget 30000000) (gap 15) (fast 64) (min 20))))
  (forget (fade questions 1)))"#;

/// A frugal maker: few features, short keys, a small budget, storage costs it more.
pub const FRUGAL_MAKER: &str = r#"
(tool "frugal-maker"
  (slot questions (key field kind int entity) count int 16)
  (slot log (key) (ring 1024) int 1)
  (slot lib (key) (votes 4) int 1)
  (observe
    (if (= kind asked) (put questions (want from given who) 0 cost))
    (if (!= kind asked) (push log)))
  (ask (make questions log lib
    (grammar (feats who (/ tod 4) prev place goal) (cells last (votes 2)) (keys 2) (cap 128) (per-kb 200) (per-fuel 10) (budget 3000000) (gap 30) (fast 64) (min 10))))
  (forget (fade questions 1)))"#;

pub fn load(src: &str) -> Desc { parse(src).unwrap_or_else(|e| panic!("{e}\n{src}")) }
