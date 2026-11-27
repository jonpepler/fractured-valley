//! The tool-maker's search: the one primitive (`make`) behind the language's `(make ...)` answer.
//!
//! The maker is itself a tool (`kit::MAKER`): its slots are the questions the mind keeps
//! asking (a count of thinking cost per question: the field wanted, the kind of record that
//! answers it, the fields given, the subject), a ring of what it has seen and done, and a
//! library of the shapes that won before. Its grammar (which features it may key on, which
//! cells, how long a key, back-off, maps, what storage and fuel cost it, its budget) is data in
//! its own description, so two makers can search differently.
//!
//! The search, when asked (in sleep):
//! 1. **Target**: the question that cost most thinking.
//! 2. **Candidates**, enumerated smallest first: tables keyed on every set of up to `max_key`
//!    usable features (features of the given fields, the time, and for a question about someone
//!    what came before), with each allowed cell, with and without back-off; and, if the grammar
//!    allows maps and the question is "which place next, from here, towards a goal", **maps**:
//!    an adjacency of places walked (one way or both ways, with or without a slot of where
//!    surprises happened, to keep away from), answered by shortest path, alone or behind a
//!    journeys table. Shapes from the library are tried first (DreamCoder-lite).
//! 3. **Score** each on the mind's own history, prequentially: replay the ring in order; at each
//!    record that answers the question, ask the candidate first, then tell it. Score = hits per
//!    mille − storage and fuel penalties. Every replay runs in the same interpreter, paid for
//!    from the maker's own fuel, so the search is deterministic and bounded.
//! 4. The best (ties: smaller, then by shape hash) is returned; its shape gets a library vote.
//!
//! Hand-written in the builder: a record with a negative outcome (`val` < 0: a blocked step, a
//! failure) drops what it says (first version: counted −32 against it, too slow to undo a habit);
//! every other record counts +1; tables fade by 1/8 a
//! night, maps by 1/16, the surprise slot by 1/2.

use super::lang::*;
use super::run::{Fuel, Res, Spent, St, Tool};

/// What the maker made, and how it scored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Made { pub score: i32, pub acc: i32, pub bytes: i32, pub fuel_per_ask: i32, pub asked: i32, pub tried: u32, pub of: u32,
    pub want: i32, pub from: i32, pub given: i32, pub who: i32, pub from_lib: bool, pub map: bool }

/// A candidate's shape.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Shape {
    Table { feats: Vec<Feat>, cell: Cell, back: bool },
    /// An adjacency map with shortest path; `both`: links count both ways; `avoid`: keep away
    /// from where surprises were; `journeys`: first a (to, place) table of journeys made.
    Map { both: bool, avoid: bool, journeys: bool },
}

pub const AVOID_AT: i32 = 300;

fn failed() -> E { E::Op(Op::Lt, Box::new(E::F(F::Val)), Box::new(E::C(0))) }
fn ok() -> E { E::Op(Op::Or, Box::new(E::Op(Op::Eq, Box::new(E::F(F::Val)), Box::new(E::C(NONE)))), Box::new(E::Op(Op::Le, Box::new(E::C(0)), Box::new(E::F(F::Val))))) }
/// Learn a record into a slot: +1 for what happened; a failure (val < 0) drops what it says.
fn learn(i: u8, k: Vec<E>, v: E) -> Vec<S> { vec![S::If(ok(), vec![S::Put(i, k.clone(), v.clone(), E::C(1))]), S::If(failed(), vec![S::Drop(i, k, v)])] }
fn is_kind(k: i32) -> E { E::Op(Op::Eq, Box::new(E::F(F::Kind)), Box::new(E::C(k))) }

fn feat_expr(f: Feat, last: u8, want: F) -> (E, Ty) {
    let get_last = |rd| E::Get(last, vec![E::F(F::Who)], rd);
    match f {
        Feat::Field(x) => (E::F(x), x.ty()),
        Feat::Div(x, n) => (E::Op(Op::Div, Box::new(E::F(x)), Box::new(E::C(n))), x.ty()),
        Feat::Mod(x, n) => (E::Op(Op::Mod, Box::new(E::F(x)), Box::new(E::C(n))), x.ty()),
        Feat::Prev => (get_last(Rd::Top(0)), want.ty()),
        Feat::Since(n) => (E::Op(Op::Min, Box::new(E::Op(Op::Div, Box::new(E::Op(Op::Sub, Box::new(E::F(F::Tick)), Box::new(get_last(Rd::At)))), Box::new(E::C(n)))), Box::new(E::C(7))), Ty::Int),
    }
}

/// Build a candidate description.
pub fn build(want: F, from: i32, shape: &Shape, cap: u16) -> Desc {
    match shape {
        Shape::Table { feats, cell, back } => {
            let needs_last = feats.iter().any(|f| matches!(f, Feat::Prev | Feat::Since(_)));
            let back = *back && feats.len() >= 2;
            let last = if back { 2u8 } else { 1u8 };
            let keys: Vec<(E, Ty)> = feats.iter().map(|&f| feat_expr(f, last, want)).collect();
            let mut slots = vec![Slot { name: "table".into(), key: keys.iter().map(|k| k.1).collect(), cell: *cell, cap, val: want.ty() }];
            if back { slots.push(Slot { name: "back".into(), key: keys[..keys.len() - 1].iter().map(|k| k.1).collect(), cell: *cell, cap, val: want.ty() }); }
            if needs_last { slots.push(Slot { name: "last".into(), key: vec![Ty::Entity], cell: Cell::Last, cap, val: want.ty() }); }
            let ke: Vec<E> = keys.iter().map(|k| k.0.clone()).collect();
            let mut body = learn(0, ke.clone(), E::F(want));
            if back { body.extend(learn(1, ke[..ke.len() - 1].to_vec(), E::F(want))); }
            if needs_last { body.push(S::Put(last, vec![E::F(F::Who)], E::F(want), E::C(1))); }
            let top = |i: u8, k: Vec<E>| A::Top(i, k, 3);
            let ask = if back { A::First(vec![top(0, ke.clone()), top(1, ke[..ke.len() - 1].to_vec())]) } else { top(0, ke) };
            let mut forget = vec![];
            if matches!(cell, Cell::Votes(_) | Cell::Hist(_)) { forget.push(S::Fade(0, 3)); if back { forget.push(S::Fade(1, 3)); } }
            let name = format!("made({}{})", feats.iter().map(|&f| feat_text(f)).collect::<Vec<_>>().join(" "), if back { " backoff" } else { "" });
            Desc { name, slots, observe: vec![S::If(is_kind(from), body)], ask, forget, uses: vec![], views: vec![] }
        }
        Shape::Map { both, avoid, journeys } => {
            let mut slots = vec![Slot { name: "map".into(), key: vec![Ty::Place], cell: Cell::Votes(4), cap, val: Ty::Place }];
            let mut body = learn(0, vec![E::F(F::Place)], E::F(want));
            if *both { body.extend(learn(0, vec![E::F(want)], E::F(F::Place))); }
            let mut observe = vec![];
            let mut forget = vec![S::Fade(0, 4)];
            let av = if *avoid {
                slots.push(Slot { name: "trouble".into(), key: vec![Ty::Place], cell: Cell::Count, cap, val: Ty::Int });
                let i = slots.len() as u8 - 1;
                observe.push(S::If(is_kind(kind::SURPRISE), vec![S::Put(i, vec![E::F(F::Place)], E::C(0), E::F(F::Val))]));
                forget.push(S::Fade(i, 1));
                Some((i, AVOID_AT))
            } else { None };
            let path = A::Path(0, E::F(F::Place), E::F(F::Goal), 16, av);
            let ask = if *journeys {
                slots.push(Slot { name: "journeys".into(), key: vec![Ty::Place, Ty::Place], cell: Cell::Votes(2), cap, val: Ty::Place });
                let j = slots.len() as u8 - 1;
                body.extend(learn(j, vec![E::F(F::Goal), E::F(F::Place)], E::F(want)));
                forget.push(S::Fade(j, 3));
                A::First(vec![A::Top(j, vec![E::F(F::Goal), E::F(F::Place)], 2), path])
            } else { path };
            observe.insert(0, S::If(is_kind(from), body));
            let name = format!("made(map{}{}{})", if *both { " both-ways" } else { " one-way" }, if *avoid { " avoid" } else { "" }, if *journeys { " journeys-first" } else { "" });
            // Views, so other tools can build on the map (9.10, the owner: a map is a building block):
            // the way to a goal, and a place's neighbours.
            let views = vec![("route".to_string(), A::Path(0, E::F(F::Place), E::F(F::Goal), 16, av)), ("near".to_string(), A::Top(0, vec![E::F(F::Place)], 4))];
            Desc { name, slots, observe, ask, forget, uses: vec![], views }
        }
    }
}

/// A shape's code for the library (a positive i32).
pub fn shape_code(s: &Shape) -> i32 {
    let mut h = crate::rng::StateHasher::new(0x70c0);
    std::hash::Hash::hash(s, &mut h);
    (std::hash::Hasher::finish(&h) & 0x3fff_ffff) as i32
}

const TIME: i32 = (1 << F::Tick as i32) | (1 << F::Day as i32) | (1 << F::Tod as i32);
fn has(given: i32, f: F) -> bool { (given | TIME) & (1 << f as i32) != 0 }
fn usable(f: Feat, given: i32) -> bool {
    match f { Feat::Field(x) | Feat::Div(x, _) | Feat::Mod(x, _) => has(given, x), Feat::Prev | Feat::Since(_) => has(given, F::Who) }
}

/// The query a record would have been asked as: the given fields and the time, nothing else.
pub fn as_question(r: &Rec, want: F, given: i32) -> Rec {
    let mut q = rec();
    for f in FIELDS { if has(given, f) && f != want { q[f as usize] = r[f as usize]; } }
    q[F::Want as usize] = want as i32;
    q
}

/// Does a record answer the question (and is it a time the question would be asked)?
pub struct Target { pub want: F, pub from: i32, pub given: i32, pub who: i32, pub gap: i32 }
impl Target {
    pub fn answers(&self, r: &Rec) -> bool {
        r[F::Kind as usize] == self.from && r[self.want as usize] != NONE && (r[F::Val as usize] == NONE || r[F::Val as usize] >= 0) && (self.who == NONE || r[F::Who as usize] == self.who)
    }
}

/// Score a candidate on a log of records, prequentially. Returns (hits, asked, ask fuel, tool).
pub fn replay(d: &Desc, log: &[Rec], t: &Target, fuel: &mut Fuel) -> Res<(i32, i32, i64, Tool)> {
    let mut tool = Tool::from(d.clone());
    let (mut hits, mut asked, mut ask_fuel) = (0, 0, 0i64);
    let mut last_seen: Vec<(i32, i32)> = vec![];
    let mut day = NONE;
    for r in log {
        fuel.spend(1)?;
        if r[F::Day as usize] != day { if day != NONE { let used = tool.forget(r[F::Tick as usize], fuel.left).map_err(|_| Spent)?; fuel.spend(used)?; } day = r[F::Day as usize]; }
        let w = r[F::Who as usize];
        let fresh = w == NONE || match last_seen.iter_mut().find(|p| p.0 == w) { Some(p) => { let f = r[F::Tick as usize] - p.1 >= t.gap; p.1 = r[F::Tick as usize]; f } None => { last_seen.push((w, r[F::Tick as usize])); true } };
        if fresh && t.answers(r) {
            let q = as_question(r, t.want, t.given);
            let mut f = Fuel::new(fuel.left.min(10_000));
            let a = tool.answer_only(&q, &mut f);
            fuel.spend(f.used)?;
            let a = a?;
            asked += 1; ask_fuel += f.used;
            if a.first().is_some_and(|x| x.0 == r[t.want as usize]) { hits += 1; }
        }
        let used = tool.observe(r, fuel.left.min(10_000)).map_err(|_| Spent)?;
        fuel.spend(used)?;
    }
    Ok((hits, asked, ask_fuel, tool))
}

/// All sets of up to `k` of `n` items, as index lists, smallest first, then in order.
pub fn subsets(n: usize, k: usize) -> Vec<Vec<usize>> {
    let mut out = vec![];
    for size in 1..=k.min(n) {
        let mut idx: Vec<usize> = (0..size).collect();
        loop {
            out.push(idx.clone());
            let mut i = size;
            while i > 0 && idx[i - 1] == n - size + i - 1 { i -= 1; }
            if i == 0 { break; }
            idx[i - 1] += 1;
            for j in i..size { idx[j] = idx[j - 1] + 1; }
        }
    }
    out
}

/// The candidates for a question under a grammar, library shapes first.
pub fn candidates(g: &Grammar, want: F, given: i32, lib: &[i32]) -> Vec<Shape> {
    let feats: Vec<Feat> = g.feats.iter().copied().filter(|&f| usable(f, given)).collect();
    let mut out = vec![];
    for s in subsets(feats.len(), g.max_key as usize) {
        let fs: Vec<Feat> = s.iter().map(|&i| feats[i]).collect();
        for &c in &g.cells { out.push(Shape::Table { feats: fs.clone(), cell: c, back: false }); if g.backoff && fs.len() >= 2 { out.push(Shape::Table { feats: fs.clone(), cell: c, back: true }); } }
    }
    if g.paths && want.ty() == Ty::Place && has(given, F::Place) && has(given, F::Goal) {
        for both in [false, true] { for avoid in [false, true] { for journeys in [false, true] { out.push(Shape::Map { both, avoid, journeys }); } } }
    }
    // Stable: library shapes first, then as enumerated.
    let mut keyed: Vec<(usize, usize, Shape)> = out.into_iter().enumerate().map(|(i, s)| (lib.iter().position(|&c| c == shape_code(&s)).unwrap_or(usize::MAX), i, s)).collect();
    keyed.sort_by_key(|x| (x.0, x.1));
    keyed.into_iter().map(|x| x.2).collect()
}

/// The search. `qs`: the questions slot (count, keyed (want, from, given, who)); `log`: the ring;
/// `lib`: the library slot (votes over shape codes, no key).
pub fn make(t: &mut Tool, qs: u8, log: u8, lib: u8, g: &Grammar, fuel: &mut Fuel) -> Res<Option<(Desc, Made, Option<(Desc, Made)>)>> {
    let Some(St::T(q)) = t.slots.get(qs as usize) else { return Ok(None) };
    fuel.spend(q.keys.len() as i64 + 1)?;
    let Some((key, _)) = q.keys.iter().zip(&q.cells).map(|(k, c)| (*k, c[0])).filter(|x| x.1 > 0).max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0))) else { return Ok(None) };
    let Some(want) = F::from_u8(key[0].clamp(0, 255) as u8) else { return Ok(None) };
    let target = Target { want, from: key[1], given: key[2], who: if key[3] == ANYONE { NONE } else { key[3] }, gap: g.gap };
    let Some(St::R(ring)) = t.slots.get(log as usize) else { return Ok(None) };
    let recs: Vec<Rec> = ring.iter().copied().collect();
    fuel.spend(recs.len() as i64 / 8 + 1)?;
    let lib_codes: Vec<i32> = match t.slots.get(lib as usize) { Some(St::T(l)) => l.cells.first().map(|c| (0..4).filter(|&j| c[2 * j + 1] > 0).map(|j| c[2 * j]).collect()).unwrap_or_default(), _ => vec![] };
    let cands = candidates(g, want, target.given, &lib_codes);
    let of = cands.len() as u32;
    let mut best: Option<(Desc, Made, i32)> = None;
    let mut cheap: Option<(Desc, Made)> = None;
    let mut tried = 0;
    for s in cands {
        let d = build(want, target.from, &s, g.cap);
        let Ok((hits, asked, af, tool)) = replay(&d, &recs, &target, fuel) else { break };
        tried += 1;
        if asked == 0 || asked < g.min { continue; }
        let acc = hits * 1000 / asked;
        let bytes = (tool.bytes() + d.size()) as i32;
        let fpa = (af / asked as i64) as i32;
        let score = acc - g.per_kb * bytes / 1024 - g.per_fuel * fpa / 10;
        let code = shape_code(&s);
        let m = Made { score, acc, bytes, fuel_per_ask: fpa, asked, tried, of, want: want as i32, from: target.from, given: target.given, who: target.who, from_lib: lib_codes.contains(&code), map: matches!(s, Shape::Map { .. }) };
        let better = match &best { None => true, Some((bd, bm, _)) => (score, -bytes, std::cmp::Reverse(d.shape())) > (bm.score, -bm.bytes, std::cmp::Reverse(bd.shape())) };
        // The cheap version: the best whose static bound fits the fast brain.
        if g.fast > 0 && super::cost::bound(&d, &[]).ask <= g.fast as i64 {
            let b2 = match &cheap { None => true, Some((bd, bm)) => (score, -bytes, std::cmp::Reverse(d.shape())) > (bm.score, -bm.bytes, std::cmp::Reverse(bd.shape())) };
            if b2 { cheap = Some((d.clone(), m)); }
        }
        if better { best = Some((d, m, code)); }
    }
    let Some((d, mut m, code)) = best else { return Ok(None) };
    m.tried = tried;
    if let Some(St::T(l)) = t.slots.get_mut(lib as usize) {
        if l.keys.is_empty() { l.keys.push([NONE; MAX_KEY]); l.cells.push([NONE, 0, NONE, 0, NONE, 0, NONE, 0]); }
        let c = &mut l.cells[0];
        if let Some(j) = (0..4).find(|&j| c[2 * j] == code) { c[2 * j + 1] += 1; } else if let Some(j) = (0..4).find(|&j| c[2 * j + 1] <= 0) { c[2 * j] = code; c[2 * j + 1] = 1; } else { let j = (0..4).min_by_key(|&j| (c[2 * j + 1], j)).unwrap(); c[2 * j] = code; c[2 * j + 1] += 1; }
    }
    let cheap = cheap.filter(|c| c.0.shape() != d.shape()).map(|(cd, mut cm)| { cm.tried = tried; (cd, cm) });
    Ok(Some((d, m, cheap)))
}

/// Parse a `(grammar ...)` on its own.
pub fn parse_grammar(src: &str) -> Result<Grammar, CheckError> {
    let d = parse(&format!("(tool \"g\" (slot q (key) count int 1) (slot l (key) (ring 1) int 1) (ask (make q l q {src})))"))?;
    match d.ask { A::Make(_, _, _, g) => Ok(*g), _ => Err(CheckError("not a grammar".into())) }
}

/// The bit mask of a set of fields (for `given`).
pub fn mask(fs: &[F]) -> i32 { fs.iter().fold(0, |m, &f| m | 1 << f as i32) }
