//! The tool description language: a tool is **data**, not Rust code.
//!
//! A description (`Desc`) has
//! - **slots**: its state, each a keyed table of one kind of **cell** (`Cell`: top-k votes, last
//!   value, a count, a small histogram) or a ring buffer of records;
//! - **observe**: statements run on each record told to it (`S`): `if`, `put`, `push`;
//! - **ask**: one answer expression (`A`) run on a query record, giving ranked `(value, score)`s;
//! - **forget**: statements run when the mind sleeps (`fade`).
//!
//! Every value is an `i32`. A record (`Rec`) has a fixed set of fields, the things a mind has to
//! hand: the kind of record, the tick, the day and time-of-day bucket, an entity, a place, a cell
//! position, an action, a feeling, a node, a listener, and (for questions) the field wanted and
//! what thinking it through cost. Missing is `NONE`, and `NONE` spreads through arithmetic, so a
//! `put` with a missing key or value does nothing: no special cases for partial records.
//!
//! Descriptions are typed (`check`), hashable and serialisable (`encode`/`decode`, a few dozen
//! bytes), printable and parseable as s-expressions (`Display`, `parse`), and can be put into
//! words (`explain`), so a mind could tell or write one down (9.10).

use crate::rng::StateHasher;
use std::fmt::{self, Write as _};
use std::hash::Hasher;

/// Missing.
pub const NONE: i32 = i32::MIN;
/// "Anyone": a subject key for a question about no one in particular.
pub const ANYONE: i32 = -1;

/// The fields of a record. Order is the binary encoding: append only.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum F { Kind = 0, Tick, Day, Tod, Who, Place, X, Y, Act, Val, What, To, Want, Cost, Given, From, Next, Goal }
pub const NF: usize = 18;
pub const FIELDS: [F; NF] = [F::Kind, F::Tick, F::Day, F::Tod, F::Who, F::Place, F::X, F::Y, F::Act, F::Val, F::What, F::To, F::Want, F::Cost, F::Given, F::From, F::Next, F::Goal];
pub const FIELD_NAMES: [&str; NF] = ["kind", "tick", "day", "tod", "who", "place", "x", "y", "act", "val", "what", "to", "want", "cost", "given", "from", "next", "goal"];

/// The types of values. A key position has a type; a put or get must give it a value of that type.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Ty { Int = 0, Kind, Tick, Day, Tod, Entity, Place, Coord, Act, Feel, Node, Field }
pub const TY_NAMES: [&str; 12] = ["int", "kind", "tick", "day", "tod", "entity", "place", "coord", "act", "feel", "node", "field"];
pub const TYS: [Ty; 12] = [Ty::Int, Ty::Kind, Ty::Tick, Ty::Day, Ty::Tod, Ty::Entity, Ty::Place, Ty::Coord, Ty::Act, Ty::Feel, Ty::Node, Ty::Field];

impl F {
    pub fn ty(self) -> Ty {
        match self { F::Kind => Ty::Kind, F::Tick => Ty::Tick, F::Day => Ty::Day, F::Tod => Ty::Tod, F::Who | F::To => Ty::Entity, F::Place => Ty::Place,
            F::X | F::Y => Ty::Coord, F::Act => Ty::Act, F::Val => Ty::Feel, F::What => Ty::Node, F::Want => Ty::Field, F::Next | F::Goal => Ty::Place, F::Cost | F::Given => Ty::Int, F::From => Ty::Kind }
    }
    pub fn name(self) -> &'static str { FIELD_NAMES[self as usize] }
    pub fn from_u8(b: u8) -> Option<F> { FIELDS.get(b as usize).copied() }
}

/// Kinds of record (the `kind` field).
pub mod kind {
    /// Someone (`who`) seen at a place (`place`, `x`, `y`).
    pub const SEEN: i32 = 1;
    /// I did `act` at `place`; it went `val` (−1000..1000).
    pub const DID: i32 = 2;
    /// `who` told `to` about `what`.
    pub const TOLD: i32 = 3;
    /// A surprise about `what` (or `who`), of size `val`.
    pub const SURPRISE: i32 = 4;
    /// A question I thought through: the field `want`, answered by records of kind `from`, given
    /// the fields in the mask `given` (bit = field number), about `who` (or none), costing `cost`
    /// link reads.
    pub const ASKED: i32 = 5;
    /// I moved from `place` to the next place `next`, on the way to `goal` (`val` < 0: I tried, and
    /// the way was blocked).
    pub const MOVED: i32 = 6;
    pub const NAMES: [&str; 7] = ["none", "seen", "did", "told", "surprise", "asked", "moved"];
}

/// A record: what a tool observes, and what it is asked.
pub type Rec = [i32; NF];
pub fn rec() -> Rec { [NONE; NF] }
/// Build a record from (field, value) pairs.
pub fn rec_of(fs: &[(F, i32)]) -> Rec { let mut r = rec(); for &(f, v) in fs { r[f as usize] = v; } r }

/// Binary operators.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
#[repr(u8)]
pub enum Op { Add = 0, Sub, Mul, Div, Mod, Min, Max, Eq, Ne, Lt, Le, And, Or }
pub const OP_NAMES: [&str; 13] = ["+", "-", "*", "/", "%", "min", "max", "=", "!=", "<", "<=", "and", "or"];
pub const OPS: [Op; 13] = [Op::Add, Op::Sub, Op::Mul, Op::Div, Op::Mod, Op::Min, Op::Max, Op::Eq, Op::Ne, Op::Lt, Op::Le, Op::And, Op::Or];

/// What to read from a cell.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum Rd {
    /// The value ranked `n` (0: the best).
    Top(u8),
    /// The weight of the value ranked `n`.
    W(u8),
    /// The total weight (a count's sum).
    Count,
    /// When it was last written (the record's tick).
    At,
    /// 1 if the entry exists, else 0.
    Has,
}

/// An expression: an integer from the record at hand, constants and the tool's own state.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum E {
    C(i32),
    F(F),
    /// The current value of a `chain` (the place reached so far).
    It,
    Op(Op, Box<E>, Box<E>),
    If(Box<E>, Box<E>, Box<E>),
    /// Read a cell of a slot by key.
    Get(u8, Vec<E>, Rd),
    /// Round-2 revision (composition): ask another of the mind's tools (by its place in `uses`),
    /// through one of its views ("" for its main answer), with a query built here; the value
    /// ranked `n`. Its cost comes out of this call's fuel.
    Sub(u8, String, Vec<(F, E)>, u8),
}

/// What a slot's entries hold.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum Cell {
    /// The `k` (≤ 4) values with most weight (space-saving when full; weights may be negative).
    Votes(u8),
    /// The last value put, and when.
    Last,
    /// A running total of weights, and how many puts.
    Count,
    /// `n` (≤ 8) bins of weight over values 0..n (clamped).
    Hist(u8),
    /// A ring buffer of the last `len` whole records (no key).
    Ring(u16),
}
pub const MAX_KEY: usize = 4;
pub const MAX_VOTES: u8 = 4;
pub const MAX_BINS: u8 = 8;

/// A slot: a named, keyed table of cells, with at most `cap` entries (the weakest is evicted).
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Slot { pub name: String, pub key: Vec<Ty>, pub cell: Cell, pub cap: u16, /// The type of the values it holds.
    pub val: Ty }

/// A statement (observe and forget).
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum S {
    If(E, Vec<S>),
    /// put slot key value weight
    Put(u8, Vec<E>, E, E),
    /// Push the record at hand into a ring.
    Push(u8),
    /// Every weight in a slot loses 1/2^shift of itself (at least 1); empty entries go.
    Fade(u8, u8),
    /// Round-2 revision (routes): drop one value from a cell (a blocked way is no longer a way:
    /// counting against it was too slow to undo a strong habit).
    Drop(u8, Vec<E>, E),
}

/// An answer: ranked (value, score) pairs, best first.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum A {
    /// The top `k` of a slot's cell at a key.
    Top(u8, Vec<E>, u8),
    /// One value (if not missing), score 1000.
    Val(E),
    /// The first answer that is not empty (back-off).
    First(Vec<A>),
    /// Each answer's scores as shares of its total, times its weight, summed by value.
    Blend(Vec<(i32, A)>),
    If(E, Box<A>, Box<A>),
    /// Start at a value and follow a slot's best vote `steps` times (≤ 16), with `it` the value
    /// so far in the key: a route or sequence followed forward.
    Chain(u8, Vec<E>, E, E),
    /// Round-2 revision (routes): the first step of a shortest path from `from` to `goal` over a
    /// slot used as a map (its votes are the neighbours), at most `max` steps; with an avoid
    /// slot, places whose count there is at least the threshold are not passed through. Score
    /// 1000 less 10 per step.
    Path(u8, E, E, u8, Option<(u8, i32)>),
    /// Round-2 revision (composition): another tool's whole answer (see `E::Sub`).
    Sub(u8, String, Vec<(F, E)>),
    /// The tool-maker's search (see `maker`): from the slot of questions asked, the ring of
    /// records, and the library slot, under the grammar.
    Make(u8, u8, u8, Box<Grammar>),
}

/// A feature a tool-maker may key a table on.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum Feat {
    Field(F),
    /// field / n (coarser buckets)
    Div(F, i32),
    /// field % n (a cycle: day % 3)
    Mod(F, i32),
    /// The last value of the wanted field for this `who` (what came before: a sequence).
    Prev,
    /// Ticks since this `who` was last seen, / n, at most 7 (a dwell clock).
    Since(i32),
}

/// What a tool-maker may build: its search space, its scoring and its budget. Data, so two makers
/// can search differently (character).
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Grammar {
    pub feats: Vec<Feat>,
    pub cells: Vec<Cell>,
    /// At most this many features in a key.
    pub max_key: u8,
    /// Also try back-off: the key, then the key without its last feature.
    pub backoff: bool,
    pub cap: u16,
    /// Score = hits per mille − `per_kb` × KB stored − `per_fuel` × fuel per ask / 10.
    pub per_kb: i32,
    pub per_fuel: i32,
    /// Fuel for one search.
    pub budget: i32,
    /// Only score a sighting this many ticks after the last one of the same `who` (a question is
    /// asked when they are out of sight).
    pub gap: i32,
    /// Round-2 revision (routes): also try maps (an adjacency slot and `path`), when the
    /// question is about a place and gives `place` and `to`.
    pub paths: bool,
    /// Owner's addition (reach): also return the best candidate whose static ask bound is at
    /// most this (a cheap version for the fast brain); 0: don't.
    pub fast: i32,
    /// Make nothing until the question has come up at least this many times in the history.
    pub min: i32,
}

/// A tool description.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Desc { pub name: String, pub slots: Vec<Slot>, pub observe: Vec<S>, pub ask: A, pub forget: Vec<S>,
    /// Round-2 revision (composition): the other tools (by name) this one may ask, and further
    /// named answers ("views") other tools may ask this one for.
    pub uses: Vec<String>, pub views: Vec<(String, A)> }

// ------------------------------------------------------------------------------- checking

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckError(pub String);
impl fmt::Display for CheckError { fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { f.write_str(&self.0) } }

fn compat(a: Ty, b: Ty) -> bool { a == b || a == Ty::Int || b == Ty::Int }

impl Desc {
    /// Type-check: slots exist, key lengths and types match, cells are within bounds.
    pub fn check(&self) -> Result<(), CheckError> {
        let err = |s: String| Err(CheckError(format!("{}: {s}", self.name)));
        if self.slots.len() > 32 { return err("more than 32 slots".into()); }
        for s in &self.slots {
            if s.key.len() > MAX_KEY { return err(format!("slot {} has a key longer than {MAX_KEY}", s.name)); }
            match s.cell { Cell::Votes(k) if k == 0 || k > MAX_VOTES => return err(format!("slot {}: votes 1..{MAX_VOTES}", s.name)),
                Cell::Hist(n) if n == 0 || n > MAX_BINS => return err(format!("slot {}: hist 1..{MAX_BINS}", s.name)),
                Cell::Ring(n) if n == 0 || !s.key.is_empty() => return err(format!("slot {}: a ring has no key and a length", s.name)), _ => {} }
            if s.cap == 0 { return err(format!("slot {} has cap 0", s.name)); }
        }
        for st in self.observe.iter().chain(&self.forget) { self.check_s(st)?; if s_has_sub(st) { return err("a sub-tool can only be asked from an answer".into()); } }
        if self.uses.len() > 8 || self.views.len() > 8 { return err("at most 8 uses and 8 views".into()); }
        for (_, v) in &self.views { self.check_a(v)?; }
        self.check_a(&self.ask)
    }
    fn slot(&self, i: u8) -> Result<&Slot, CheckError> { self.slots.get(i as usize).ok_or_else(|| CheckError(format!("{}: no slot {i}", self.name))) }
    fn check_key(&self, i: u8, key: &[E]) -> Result<&Slot, CheckError> {
        let s = self.slot(i)?;
        if matches!(s.cell, Cell::Ring(_)) { return Err(CheckError(format!("{}: slot {} is a ring", self.name, s.name))); }
        if key.len() != s.key.len() { return Err(CheckError(format!("{}: slot {} takes {} keys, given {}", self.name, s.name, s.key.len(), key.len()))); }
        for (k, (e, &t)) in key.iter().zip(&s.key).enumerate() {
            let te = self.ty_e(e)?;
            if !compat(te, t) { return Err(CheckError(format!("{}: slot {} key {k} is {}, given {}", self.name, s.name, TY_NAMES[t as usize], TY_NAMES[te as usize]))); }
        }
        Ok(s)
    }
    /// The type of an expression (arithmetic keeps its left operand's type; comparisons are ints).
    pub fn ty_e(&self, e: &E) -> Result<Ty, CheckError> {
        Ok(match e {
            E::C(_) | E::It => Ty::Int,
            E::F(f) => f.ty(),
            E::Op(op, a, b) => { let (ta, tb) = (self.ty_e(a)?, self.ty_e(b)?); match op { Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::And | Op::Or => Ty::Int, _ => if ta == Ty::Int { tb } else { ta } } }
            E::If(c, a, b) => { self.ty_e(c)?; let (ta, tb) = (self.ty_e(a)?, self.ty_e(b)?); if compat(ta, tb) { if ta == Ty::Int { tb } else { ta } } else { return Err(CheckError(format!("{}: if branches differ", self.name))); } }
            E::Get(i, key, rd) => { let s = self.check_key(*i, key)?; match rd { Rd::Top(_) => s.val, Rd::At => Ty::Tick, _ => Ty::Int } }
            E::Sub(u, _, q, _) => { self.check_use(*u)?; for (_, e) in q { self.ty_e(e)?; } Ty::Int }
        })
    }
    fn check_use(&self, u: u8) -> Result<(), CheckError> { if (u as usize) < self.uses.len() { Ok(()) } else { Err(CheckError(format!("{}: no sub-tool {u}", self.name))) } }
    fn check_s(&self, st: &S) -> Result<(), CheckError> {
        match st {
            S::If(c, b) => { self.ty_e(c)?; for s in b { self.check_s(s)?; } Ok(()) }
            S::Put(i, key, v, w) => { let s = self.check_key(*i, key)?; let tv = self.ty_e(v)?; self.ty_e(w)?;
                if !matches!(s.cell, Cell::Count) && !compat(tv, s.val) { return Err(CheckError(format!("{}: slot {} holds {}, given {}", self.name, s.name, TY_NAMES[s.val as usize], TY_NAMES[tv as usize]))); } Ok(()) }
            S::Push(i) => { let s = self.slot(*i)?; if !matches!(s.cell, Cell::Ring(_)) { return Err(CheckError(format!("{}: push to {} which is not a ring", self.name, s.name))); } Ok(()) }
            S::Fade(i, sh) => { self.slot(*i)?; if *sh == 0 || *sh > 16 { return Err(CheckError(format!("{}: fade shift 1..16", self.name))); } Ok(()) }
            S::Drop(i, key, v) => { let s = self.check_key(*i, key)?; let tv = self.ty_e(v)?; if !compat(tv, s.val) { return Err(CheckError(format!("{}: drop from {}: holds {}, given {}", self.name, s.name, TY_NAMES[s.val as usize], TY_NAMES[tv as usize]))); } Ok(()) }
        }
    }
    fn check_a(&self, a: &A) -> Result<(), CheckError> {
        match a {
            A::Top(i, key, k) => { self.check_key(*i, key)?; if *k == 0 || *k > 8 { return Err(CheckError(format!("{}: top 1..8", self.name))); } Ok(()) }
            A::Val(e) => { self.ty_e(e)?; Ok(()) }
            A::First(v) => { for x in v { self.check_a(x)?; } Ok(()) }
            A::Blend(v) => { for (_, x) in v { self.check_a(x)?; } Ok(()) }
            A::If(c, x, y) => { self.ty_e(c)?; self.check_a(x)?; self.check_a(y) }
            A::Chain(i, key, s, n) => { self.check_key(*i, key)?; self.ty_e(s)?; self.ty_e(n)?; Ok(()) }
            A::Path(i, a, b, n, av) => { let s = self.check_key(*i, &[E::C(0)])?; if s.val != Ty::Place && s.val != Ty::Int { return Err(CheckError(format!("{}: path needs a map of places", self.name))); } self.ty_e(a)?; self.ty_e(b)?; if *n == 0 || *n > 32 { return Err(CheckError(format!("{}: path length 1..32", self.name))); } if let Some((j, _)) = av { self.check_key(*j, &[E::C(0)])?; } Ok(()) }
            A::Sub(u, _, q) => { self.check_use(*u)?; for (_, e) in q { self.ty_e(e)?; } Ok(()) }
            A::Make(q, l, b, _) => { self.slot(*q)?; let ls = self.slot(*l)?; self.slot(*b)?; if !matches!(ls.cell, Cell::Ring(_)) { return Err(CheckError(format!("{}: make's log must be a ring", self.name))); } Ok(()) }
        }
    }
    /// The type of the answer's values.
    pub fn answer_ty(&self) -> Ty { self.ty_a(&self.ask).unwrap_or(Ty::Int) }
    fn ty_a(&self, a: &A) -> Option<Ty> {
        match a { A::Top(i, _, _) | A::Chain(i, _, _, _) => self.slots.get(*i as usize).map(|s| s.val), A::Val(e) => self.ty_e(e).ok(),
            A::First(v) => v.first().and_then(|x| self.ty_a(x)), A::Blend(v) => v.first().and_then(|x| self.ty_a(&x.1)), A::If(_, x, _) => self.ty_a(x), A::Path(i, ..) => self.slots.get(*i as usize).map(|s| s.val), A::Sub(..) | A::Make(..) => Some(Ty::Int) }
    }
}

fn e_has_sub(e: &E) -> bool { match e { E::Sub(..) => true, E::Op(_, a, b) => e_has_sub(a) || e_has_sub(b), E::If(c, a, b) => e_has_sub(c) || e_has_sub(a) || e_has_sub(b), E::Get(_, k, _) => k.iter().any(e_has_sub), _ => false } }
fn s_has_sub(s: &S) -> bool { match s { S::If(c, b) => e_has_sub(c) || b.iter().any(s_has_sub), S::Put(_, k, v, w) => k.iter().any(e_has_sub) || e_has_sub(v) || e_has_sub(w), S::Drop(_, k, v) => k.iter().any(e_has_sub) || e_has_sub(v), _ => false } }

// --------------------------------------------------------------------------- binary form

/// A small byte writer: tags as bytes, integers as zigzag varints.
#[derive(Default)]
pub struct Enc { pub b: Vec<u8> }
impl Enc {
    pub fn u8(&mut self, x: u8) { self.b.push(x); }
    pub fn int(&mut self, x: i64) { let mut z = ((x << 1) ^ (x >> 63)) as u64; loop { let b = (z & 0x7f) as u8; z >>= 7; if z == 0 { self.b.push(b); break; } self.b.push(b | 0x80); } }
    pub fn str(&mut self, s: &str) { self.int(s.len() as i64); self.b.extend_from_slice(s.as_bytes()); }
    fn e(&mut self, e: &E) {
        match e {
            E::C(c) => { self.u8(0); self.int(*c as i64); }
            E::F(f) => { self.u8(1); self.u8(*f as u8); }
            E::It => self.u8(2),
            E::Op(op, a, b) => { self.u8(3); self.u8(*op as u8); self.e(a); self.e(b); }
            E::If(c, a, b) => { self.u8(4); self.e(c); self.e(a); self.e(b); }
            E::Get(i, k, rd) => { self.u8(5); self.u8(*i); self.es(k); self.rd(*rd); }
            E::Sub(u, v, q, n) => { self.u8(6); self.u8(*u); self.str(v); self.fq(q); self.u8(*n); }
        }
    }
    fn fq(&mut self, q: &[(F, E)]) { self.u8(q.len() as u8); for (f, e) in q { self.u8(*f as u8); self.e(e); } }
    fn es(&mut self, v: &[E]) { self.u8(v.len() as u8); for e in v { self.e(e); } }
    fn rd(&mut self, r: Rd) { match r { Rd::Top(n) => { self.u8(0); self.u8(n); } Rd::W(n) => { self.u8(1); self.u8(n); } Rd::Count => self.u8(2), Rd::At => self.u8(3), Rd::Has => self.u8(4) } }
    fn cell(&mut self, c: Cell) { match c { Cell::Votes(k) => { self.u8(0); self.u8(k); } Cell::Last => self.u8(1), Cell::Count => self.u8(2), Cell::Hist(n) => { self.u8(3); self.u8(n); } Cell::Ring(n) => { self.u8(4); self.int(n as i64); } } }
    fn s(&mut self, s: &S) {
        match s {
            S::If(c, b) => { self.u8(0); self.e(c); self.ss(b); }
            S::Put(i, k, v, w) => { self.u8(1); self.u8(*i); self.es(k); self.e(v); self.e(w); }
            S::Push(i) => { self.u8(2); self.u8(*i); }
            S::Fade(i, sh) => { self.u8(3); self.u8(*i); self.u8(*sh); }
            S::Drop(i, k, v) => { self.u8(4); self.u8(*i); self.es(k); self.e(v); }
        }
    }
    fn ss(&mut self, v: &[S]) { self.u8(v.len() as u8); for s in v { self.s(s); } }
    fn feat(&mut self, f: Feat) { match f { Feat::Field(x) => { self.u8(0); self.u8(x as u8); } Feat::Div(x, n) => { self.u8(1); self.u8(x as u8); self.int(n as i64); } Feat::Mod(x, n) => { self.u8(2); self.u8(x as u8); self.int(n as i64); } Feat::Prev => self.u8(3), Feat::Since(n) => { self.u8(4); self.int(n as i64); } } }
    fn a(&mut self, a: &A) {
        match a {
            A::Top(i, k, n) => { self.u8(0); self.u8(*i); self.es(k); self.u8(*n); }
            A::Val(e) => { self.u8(1); self.e(e); }
            A::First(v) => { self.u8(2); self.u8(v.len() as u8); for x in v { self.a(x); } }
            A::Blend(v) => { self.u8(3); self.u8(v.len() as u8); for (w, x) in v { self.int(*w as i64); self.a(x); } }
            A::If(c, x, y) => { self.u8(4); self.e(c); self.a(x); self.a(y); }
            A::Chain(i, k, s, n) => { self.u8(5); self.u8(*i); self.es(k); self.e(s); self.e(n); }
            A::Path(i, a, b, n, av) => { self.u8(7); self.u8(*i); self.e(a); self.e(b); self.u8(*n); match av { None => self.u8(0), Some((j, t)) => { self.u8(1); self.u8(*j); self.int(*t as i64); } } }
            A::Sub(u, v, q) => { self.u8(8); self.u8(*u); self.str(v); self.fq(q); }
            A::Make(q, l, b, g) => { self.u8(6); self.u8(*q); self.u8(*l); self.u8(*b);
                self.u8(g.feats.len() as u8); for &f in &g.feats { self.feat(f); }
                self.u8(g.cells.len() as u8); for &c in &g.cells { self.cell(c); }
                self.u8(g.max_key); self.u8(g.backoff as u8 | (g.paths as u8) << 1); self.int(g.cap as i64); self.int(g.per_kb as i64); self.int(g.per_fuel as i64); self.int(g.budget as i64); self.int(g.gap as i64); self.int(g.fast as i64); self.int(g.min as i64); }
        }
    }
    pub fn desc(&mut self, d: &Desc, names: bool) {
        self.u8(2); // version (2: uses and views)
        self.str(if names { &d.name } else { "" });
        self.u8(d.slots.len() as u8);
        for s in &d.slots { self.str(if names { &s.name } else { "" }); self.u8(s.key.len() as u8); for &t in &s.key { self.u8(t as u8); } self.cell(s.cell); self.int(s.cap as i64); self.u8(s.val as u8); }
        self.ss(&d.observe); self.a(&d.ask); self.ss(&d.forget);
        self.u8(d.uses.len() as u8); for u in &d.uses { self.str(u); }
        self.u8(d.views.len() as u8); for (n, a) in &d.views { self.str(if names { n } else { n }); self.a(a); }
    }
}

pub struct Dec<'a> { b: &'a [u8], i: usize }
type R<T> = Result<T, CheckError>;
fn bad<T>(w: &str) -> R<T> { Err(CheckError(format!("decode: {w}"))) }
impl<'a> Dec<'a> {
    pub fn new(b: &'a [u8]) -> Self { Dec { b, i: 0 } }
    fn u8(&mut self) -> R<u8> { let x = *self.b.get(self.i).ok_or_else(|| CheckError("decode: short".into()))?; self.i += 1; Ok(x) }
    fn int(&mut self) -> R<i64> { let mut z: u64 = 0; let mut sh = 0; loop { let b = self.u8()?; z |= ((b & 0x7f) as u64) << sh; if b & 0x80 == 0 { break; } sh += 7; if sh > 63 { return bad("varint"); } } Ok(((z >> 1) as i64) ^ -((z & 1) as i64)) }
    fn i32(&mut self) -> R<i32> { i32::try_from(self.int()?).or_else(|_| bad("i32")) }
    fn str(&mut self) -> R<String> { let n = self.int()? as usize; let s = self.b.get(self.i..self.i + n).ok_or_else(|| CheckError("decode: short str".into()))?; self.i += n; String::from_utf8(s.to_vec()).or_else(|_| bad("utf8")) }
    fn f(&mut self) -> R<F> { F::from_u8(self.u8()?).map_or_else(|| bad("field"), Ok) }
    fn e(&mut self, depth: u32) -> R<E> {
        if depth > 64 { return bad("too deep"); }
        Ok(match self.u8()? {
            0 => E::C(self.i32()?), 1 => E::F(self.f()?), 2 => E::It,
            3 => { let o = self.u8()?; let op = *OPS.get(o as usize).ok_or_else(|| CheckError("decode: op".into()))?; E::Op(op, Box::new(self.e(depth + 1)?), Box::new(self.e(depth + 1)?)) }
            4 => E::If(Box::new(self.e(depth + 1)?), Box::new(self.e(depth + 1)?), Box::new(self.e(depth + 1)?)),
            5 => { let i = self.u8()?; let k = self.es(depth + 1)?; E::Get(i, k, self.rd()?) }
            6 => { let u = self.u8()?; let v = self.str()?; let q = self.fq(depth + 1)?; E::Sub(u, v, q, self.u8()?) }
            _ => return bad("expr"),
        })
    }
    fn fq(&mut self, depth: u32) -> R<Vec<(F, E)>> { let n = self.u8()?; (0..n).map(|_| { let f = self.f()?; Ok((f, self.e(depth)?)) }).collect() }
    fn es(&mut self, depth: u32) -> R<Vec<E>> { let n = self.u8()?; (0..n).map(|_| self.e(depth)).collect() }
    fn rd(&mut self) -> R<Rd> { Ok(match self.u8()? { 0 => Rd::Top(self.u8()?), 1 => Rd::W(self.u8()?), 2 => Rd::Count, 3 => Rd::At, 4 => Rd::Has, _ => return bad("rd") }) }
    fn cell(&mut self) -> R<Cell> { Ok(match self.u8()? { 0 => Cell::Votes(self.u8()?), 1 => Cell::Last, 2 => Cell::Count, 3 => Cell::Hist(self.u8()?), 4 => Cell::Ring(self.int()? as u16), _ => return bad("cell") }) }
    fn ty(&mut self) -> R<Ty> { TYS.get(self.u8()? as usize).copied().map_or_else(|| bad("ty"), Ok) }
    fn s(&mut self, depth: u32) -> R<S> {
        if depth > 64 { return bad("too deep"); }
        Ok(match self.u8()? {
            0 => { let c = self.e(depth)?; S::If(c, self.ss(depth + 1)?) }
            1 => { let i = self.u8()?; let k = self.es(depth)?; let v = self.e(depth)?; S::Put(i, k, v, self.e(depth)?) }
            2 => S::Push(self.u8()?), 3 => S::Fade(self.u8()?, self.u8()?),
            4 => { let i = self.u8()?; let k = self.es(depth)?; S::Drop(i, k, self.e(depth)?) }
            _ => return bad("stmt"),
        })
    }
    fn ss(&mut self, depth: u32) -> R<Vec<S>> { let n = self.u8()?; (0..n).map(|_| self.s(depth)).collect() }
    fn feat(&mut self) -> R<Feat> { Ok(match self.u8()? { 0 => Feat::Field(self.f()?), 1 => { let f = self.f()?; Feat::Div(f, self.i32()?) } 2 => { let f = self.f()?; Feat::Mod(f, self.i32()?) } 3 => Feat::Prev, 4 => Feat::Since(self.i32()?), _ => return bad("feat") }) }
    fn a(&mut self, depth: u32) -> R<A> {
        if depth > 64 { return bad("too deep"); }
        Ok(match self.u8()? {
            0 => { let i = self.u8()?; let k = self.es(depth)?; A::Top(i, k, self.u8()?) }
            1 => A::Val(self.e(depth)?),
            2 => { let n = self.u8()?; A::First((0..n).map(|_| self.a(depth + 1)).collect::<R<_>>()?) }
            3 => { let n = self.u8()?; A::Blend((0..n).map(|_| Ok((self.i32()?, self.a(depth + 1)?))).collect::<R<_>>()?) }
            4 => { let c = self.e(depth)?; let x = self.a(depth + 1)?; A::If(c, Box::new(x), Box::new(self.a(depth + 1)?)) }
            5 => { let i = self.u8()?; let k = self.es(depth)?; let s = self.e(depth)?; A::Chain(i, k, s, self.e(depth)?) }
            6 => { let (q, l, b) = (self.u8()?, self.u8()?, self.u8()?);
                let nf = self.u8()?; let feats = (0..nf).map(|_| self.feat()).collect::<R<_>>()?;
                let nc = self.u8()?; let cells = (0..nc).map(|_| self.cell()).collect::<R<_>>()?;
                let max_key = self.u8()?; let fl = self.u8()?; let (backoff, paths) = (fl & 1 != 0, fl & 2 != 0); let cap = self.int()? as u16;
                let (per_kb, per_fuel, budget, gap, fast, min) = (self.i32()?, self.i32()?, self.i32()?, self.i32()?, self.i32()?, self.i32()?);
                A::Make(q, l, b, Box::new(Grammar { feats, cells, max_key, backoff, cap, per_kb, per_fuel, budget, gap, paths, fast, min })) }
            8 => { let u = self.u8()?; let v = self.str()?; A::Sub(u, v, self.fq(depth + 1)?) }
            7 => { let i = self.u8()?; let a = self.e(depth)?; let b = self.e(depth)?; let n = self.u8()?; let av = match self.u8()? { 0 => None, _ => { let j = self.u8()?; Some((j, self.i32()?)) } }; A::Path(i, a, b, n, av) }
            _ => return bad("answer"),
        })
    }
    pub fn desc(&mut self) -> R<Desc> {
        if self.u8()? != 2 { return bad("version"); }
        let name = self.str()?;
        let n = self.u8()?;
        let mut slots = vec![];
        for _ in 0..n { let name = self.str()?; let nk = self.u8()?; let key = (0..nk).map(|_| self.ty()).collect::<R<_>>()?; let cell = self.cell()?; let cap = self.int()? as u16; let val = self.ty()?; slots.push(Slot { name, key, cell, cap, val }); }
        let observe = self.ss(0)?; let ask = self.a(0)?; let forget = self.ss(0)?;
        let nu = self.u8()?; let uses = (0..nu).map(|_| self.str()).collect::<R<_>>()?;
        let nv = self.u8()?; let views = (0..nv).map(|_| { let n = self.str()?; Ok((n, self.a(0)?)) }).collect::<R<_>>()?;
        if self.i != self.b.len() { return bad("trailing bytes"); }
        Ok(Desc { name, slots, observe, ask, forget, uses, views })
    }
}

impl Desc {
    /// The binary form (with names).
    pub fn encode(&self) -> Vec<u8> { let mut e = Enc::default(); e.desc(self, true); e.b }
    pub fn decode(b: &[u8]) -> Result<Desc, CheckError> { let d = Dec::new(b).desc()?; d.check()?; Ok(d) }
    /// Hash of the binary form, names included.
    pub fn digest(&self) -> u64 { let mut h = StateHasher::new(0x7001); h.write(&self.encode()); h.finish() }
    /// Hash of the shape alone (names left out): two tools that work the same have the same shape.
    pub fn shape(&self) -> u64 { let mut e = Enc::default(); e.desc(self, false); let mut h = StateHasher::new(0x7002); h.write(&e.b); h.finish() }
    /// Bytes of the binary form, without names (what a mind would have to hold or tell).
    pub fn size(&self) -> usize { let mut e = Enc::default(); e.desc(self, false); e.b.len() }
}

// ----------------------------------------------------------------------------- text form

/// Symbolic constants in the text form.
const CONSTS: [(&str, i32); 7] = [("none", NONE), ("seen", kind::SEEN), ("did", kind::DID), ("told", kind::TOLD), ("surprise", kind::SURPRISE), ("asked", kind::ASKED), ("moved", kind::MOVED)];

struct P<'a> { d: &'a Desc }
impl P<'_> {
    fn use_(&self, i: u8) -> String { self.d.uses.get(i as usize).map_or(format!("#{i}"), |s| s.clone()) }
    fn fq(&self, q: &[(F, E)], o: &mut String) { o.push('('); for (j, (f, e)) in q.iter().enumerate() { if j > 0 { o.push(' '); } let _ = write!(o, "({} ", f.name()); self.e(e, o); o.push(')'); } o.push(')'); }
    fn slot(&self, i: u8) -> String { self.d.slots.get(i as usize).map_or(format!("#{i}"), |s| s.name.clone()) }
    fn e(&self, e: &E, o: &mut String) {
        match e {
            E::C(c) => { if let Some((n, _)) = CONSTS.iter().find(|x| x.1 == *c && (*c == NONE)) { o.push_str(n) } else { let _ = write!(o, "{c}"); } }
            E::F(f) => o.push_str(f.name()),
            E::It => o.push_str("it"),
            E::Op(op, a, b) => { let _ = write!(o, "({} ", OP_NAMES[*op as usize]); self.e(a, o); o.push(' '); self.e(b, o); o.push(')'); }
            E::If(c, a, b) => { o.push_str("(if "); self.e(c, o); o.push(' '); self.e(a, o); o.push(' '); self.e(b, o); o.push(')'); }
            E::Get(i, k, rd) => { let _ = write!(o, "(get {} ", self.slot(*i)); self.key(k, o); o.push(' '); o.push_str(&rd_name(*rd)); o.push(')'); }
            E::Sub(u, v, q, n) => { let _ = write!(o, "(sub {} \"{v}\" ", self.use_(*u)); self.fq(q, o); let _ = write!(o, " {n})"); }
        }
    }
    fn kind_or(&self, e: &E, o: &mut String) { if let E::C(c) = e { if let Some(n) = kind::NAMES.get(*c as usize).filter(|_| *c > 0) { o.push_str(n); return; } } self.e(e, o) }
    fn cond(&self, e: &E, o: &mut String) {
        // `(= kind seen)` reads better than `(= kind 1)`.
        if let E::Op(op @ (Op::Eq | Op::Ne), a, b) = e { if **a == E::F(F::Kind) { let _ = write!(o, "({} kind ", OP_NAMES[*op as usize]); self.kind_or(b, o); o.push(')'); return; } }
        if let E::Op(op @ (Op::And | Op::Or), a, b) = e { let _ = write!(o, "({} ", OP_NAMES[*op as usize]); self.cond(a, o); o.push(' '); self.cond(b, o); o.push(')'); return; }
        self.e(e, o)
    }
    fn key(&self, k: &[E], o: &mut String) { o.push('('); for (j, e) in k.iter().enumerate() { if j > 0 { o.push(' '); } self.e(e, o); } o.push(')'); }
    fn s(&self, s: &S, ind: usize, o: &mut String) {
        let pad = "  ".repeat(ind);
        match s {
            S::If(c, b) => { let _ = write!(o, "{pad}(if "); self.cond(c, o); for x in b { o.push('\n'); self.s(x, ind + 1, o); } o.push(')'); }
            S::Put(i, k, v, w) => { let _ = write!(o, "{pad}(put {} ", self.slot(*i)); self.key(k, o); o.push(' '); self.e(v, o); o.push(' '); self.e(w, o); o.push(')'); }
            S::Push(i) => { let _ = write!(o, "{pad}(push {})", self.slot(*i)); }
            S::Fade(i, sh) => { let _ = write!(o, "{pad}(fade {} {sh})", self.slot(*i)); }
            S::Drop(i, k, v) => { let _ = write!(o, "{pad}(drop {} ", self.slot(*i)); self.key(k, o); o.push(' '); self.e(v, o); o.push(')'); }
        }
    }
    fn a(&self, a: &A, ind: usize, o: &mut String) {
        let pad = "  ".repeat(ind);
        match a {
            A::Top(i, k, n) => { let _ = write!(o, "{pad}(top {} ", self.slot(*i)); self.key(k, o); let _ = write!(o, " {n})"); }
            A::Val(e) => { let _ = write!(o, "{pad}(val "); self.e(e, o); o.push(')'); }
            A::First(v) => { let _ = write!(o, "{pad}(first"); for x in v { o.push('\n'); self.a(x, ind + 1, o); } o.push(')'); }
            A::Blend(v) => { let _ = write!(o, "{pad}(blend"); for (w, x) in v { let _ = write!(o, "\n{pad}  ({w}\n"); self.a(x, ind + 2, o); o.push(')'); } o.push(')'); }
            A::If(c, x, y) => { let _ = write!(o, "{pad}(if "); self.cond(c, o); o.push('\n'); self.a(x, ind + 1, o); o.push('\n'); self.a(y, ind + 1, o); o.push(')'); }
            A::Chain(i, k, s, n) => { let _ = write!(o, "{pad}(chain {} ", self.slot(*i)); self.key(k, o); o.push(' '); self.e(s, o); o.push(' '); self.e(n, o); o.push(')'); }
            A::Path(i, a, b, n, av) => { let _ = write!(o, "{pad}(path {} ", self.slot(*i)); self.e(a, o); o.push(' '); self.e(b, o); let _ = write!(o, " {n}"); if let Some((j, t)) = av { let _ = write!(o, " (avoid {} {t})", self.slot(*j)); } o.push(')'); }
            A::Sub(u, v, q) => { let _ = write!(o, "{pad}(sub {} \"{v}\" ", self.use_(*u)); self.fq(q, o); o.push(')'); }
            A::Make(q, l, b, g) => { let _ = write!(o, "{pad}(make {} {} {}\n{pad}  ", self.slot(*q), self.slot(*l), self.slot(*b)); grammar_text(g, o); o.push(')'); }
        }
    }
}
fn rd_name(r: Rd) -> String { match r { Rd::Top(n) => format!("top{n}"), Rd::W(n) => format!("w{n}"), Rd::Count => "count".into(), Rd::At => "at".into(), Rd::Has => "has".into() } }
pub fn cell_text(c: Cell) -> String { match c { Cell::Votes(k) => format!("(votes {k})"), Cell::Last => "last".into(), Cell::Count => "count".into(), Cell::Hist(n) => format!("(hist {n})"), Cell::Ring(n) => format!("(ring {n})") } }
pub fn feat_text(f: Feat) -> String { match f { Feat::Field(x) => x.name().into(), Feat::Div(x, n) => format!("(/ {} {n})", x.name()), Feat::Mod(x, n) => format!("(% {} {n})", x.name()), Feat::Prev => "prev".into(), Feat::Since(n) => format!("(since {n})") } }
fn grammar_text(g: &Grammar, o: &mut String) {
    o.push_str("(grammar (feats");
    for &f in &g.feats { o.push(' '); o.push_str(&feat_text(f)); }
    o.push_str(") (cells");
    for &c in &g.cells { o.push(' '); o.push_str(&cell_text(c)); }
    let _ = write!(o, ") (keys {}){}{} (cap {}) (per-kb {}) (per-fuel {}) (budget {}) (gap {}){}{})", g.max_key, if g.backoff { " backoff" } else { "" }, if g.paths { " paths" } else { "" }, g.cap, g.per_kb, g.per_fuel, g.budget, g.gap, if g.fast > 0 { format!(" (fast {})", g.fast) } else { String::new() }, if g.min > 0 { format!(" (min {})", g.min) } else { String::new() });
}

impl fmt::Display for Desc {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let p = P { d: self };
        let mut o = String::new();
        let _ = write!(o, "(tool \"{}\"", self.name);
        if !self.uses.is_empty() { o.push_str("\n  (uses"); for u in &self.uses { let _ = write!(o, " {u}"); } o.push(')'); }
        for s in &self.slots {
            let _ = write!(o, "\n  (slot {} (key", s.name);
            for t in &s.key { o.push(' '); o.push_str(TY_NAMES[*t as usize]); }
            let _ = write!(o, ") {} {} {})", cell_text(s.cell), TY_NAMES[s.val as usize], s.cap);
        }
        o.push_str("\n  (observe");
        for s in &self.observe { o.push('\n'); p.s(s, 2, &mut o); }
        o.push_str(")\n  (ask\n");
        p.a(&self.ask, 2, &mut o);
        o.push(')');
        for (n, a) in &self.views { let _ = write!(o, "\n  (view {n}\n"); p.a(a, 2, &mut o); o.push(')'); }
        if !self.forget.is_empty() { o.push_str("\n  (forget"); for s in &self.forget { o.push('\n'); p.s(s, 2, &mut o); } o.push(')'); }
        o.push(')');
        f.write_str(&o)
    }
}

/// An s-expression.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum X { Sym(String), Int(i64), Str(String), L(Vec<X>) }

fn tokens(s: &str) -> R<Vec<X>> {
    let mut stack: Vec<Vec<X>> = vec![vec![]];
    let cs: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c == ';' { while i < cs.len() && cs[i] != '\n' { i += 1; } continue; }
        if c.is_whitespace() { i += 1; continue; }
        if c == '(' { stack.push(vec![]); i += 1; continue; }
        if c == ')' { let l = stack.pop().ok_or_else(|| CheckError("parse: unbalanced )".into()))?; stack.last_mut().ok_or_else(|| CheckError("parse: unbalanced )".into()))?.push(X::L(l)); i += 1; continue; }
        if c == '"' { let mut j = i + 1; while j < cs.len() && cs[j] != '"' { j += 1; } stack.last_mut().unwrap().push(X::Str(cs[i + 1..j.min(cs.len())].iter().collect())); i = j + 1; continue; }
        let mut j = i; while j < cs.len() && !cs[j].is_whitespace() && cs[j] != '(' && cs[j] != ')' { j += 1; }
        let w: String = cs[i..j].iter().collect();
        let x = match w.parse::<i64>() { Ok(n) => X::Int(n), Err(_) => X::Sym(w) };
        stack.last_mut().unwrap().push(x);
        i = j;
    }
    if stack.len() != 1 { return Err(CheckError("parse: unbalanced (".into())); }
    Ok(stack.pop().unwrap())
}

struct Ps { slots: Vec<String>, uses: Vec<String> }
impl Ps {
    fn err<T>(&self, w: String) -> R<T> { Err(CheckError(format!("parse: {w}"))) }
    fn slot(&self, x: &X) -> R<u8> { match x { X::Sym(s) => self.slots.iter().position(|n| n == s).map(|p| p as u8).map_or_else(|| self.err(format!("no slot {s}")), Ok), _ => self.err(format!("slot name expected, got {x:?}")) } }
    fn use_(&self, x: &X) -> R<u8> { match x { X::Sym(s) => self.uses.iter().position(|n| n == s).map(|p| p as u8).map_or_else(|| self.err(format!("no sub-tool {s} in (uses ...)")), Ok), _ => self.err("sub-tool name expected".into()) } }
    fn fq(&self, x: &X) -> R<Vec<(F, E)>> {
        let X::L(l) = x else { return self.err("((field expr) ...) expected".into()) };
        l.iter().map(|p| match p { X::L(v) => match v.as_slice() { [X::Sym(f), e] => { let fi = FIELD_NAMES.iter().position(|n| n == f).map_or_else(|| self.err(format!("no field {f}")), Ok)?; Ok((FIELDS[fi], self.e(e)?)) } _ => self.err("(field expr) expected".into()) }, _ => self.err("(field expr) expected".into()) }).collect()
    }
    fn int(&self, x: &X) -> R<i64> { match x { X::Int(n) => Ok(*n), _ => self.err(format!("number expected, got {x:?}")) } }
    fn e(&self, x: &X) -> R<E> {
        Ok(match x {
            X::Int(n) => E::C(i32::try_from(*n).or_else(|_| self.err(format!("{n} is not an i32")))?),
            X::Sym(s) if s == "it" => E::It,
            X::Sym(s) => {
                if let Some(p) = FIELD_NAMES.iter().position(|n| n == s) { E::F(FIELDS[p]) }
                else if let Some((_, c)) = CONSTS.iter().find(|x| x.0 == s) { E::C(*c) }
                else { return self.err(format!("unknown name {s}")); }
            }
            X::L(l) => match l.as_slice() {
                [X::Sym(h), a, b] if OP_NAMES.contains(&h.as_str()) => E::Op(OPS[OP_NAMES.iter().position(|n| n == h).unwrap()], Box::new(self.e(a)?), Box::new(self.e(b)?)),
                [X::Sym(h), c, a, b] if h == "if" => E::If(Box::new(self.e(c)?), Box::new(self.e(a)?), Box::new(self.e(b)?)),
                [X::Sym(h), s, k, X::Sym(r)] if h == "get" => E::Get(self.slot(s)?, self.key(k)?, self.rd(r)?),
                [X::Sym(h), u, X::Str(v), q, n] if h == "sub" => E::Sub(self.use_(u)?, v.clone(), self.fq(q)?, self.int(n)? as u8),
                _ => return self.err(format!("bad expression {x:?}")),
            },
            X::Str(_) => return self.err("a string is not an expression".into()),
        })
    }
    fn rd(&self, r: &str) -> R<Rd> {
        Ok(match r { "count" => Rd::Count, "at" => Rd::At, "has" => Rd::Has,
            _ if r.starts_with("top") => Rd::Top(r[3..].parse().or_else(|_| self.err(format!("bad read {r}")))?),
            _ if r.starts_with('w') => Rd::W(r[1..].parse().or_else(|_| self.err(format!("bad read {r}")))?),
            _ => return self.err(format!("bad read {r}")) })
    }
    fn key(&self, x: &X) -> R<Vec<E>> { match x { X::L(l) => l.iter().map(|e| self.e(e)).collect(), _ => self.err("key list expected".into()) } }
    fn s(&self, x: &X) -> R<S> {
        let X::L(l) = x else { return self.err(format!("statement expected, got {x:?}")); };
        Ok(match l.as_slice() {
            [X::Sym(h), c, rest @ ..] if h == "if" => S::If(self.e(c)?, rest.iter().map(|s| self.s(s)).collect::<R<_>>()?),
            [X::Sym(h), s, k, v, w] if h == "put" => S::Put(self.slot(s)?, self.key(k)?, self.e(v)?, self.e(w)?),
            [X::Sym(h), s] if h == "push" => S::Push(self.slot(s)?),
            [X::Sym(h), s, n] if h == "fade" => S::Fade(self.slot(s)?, self.int(n)? as u8),
            [X::Sym(h), s, k, v] if h == "drop" => S::Drop(self.slot(s)?, self.key(k)?, self.e(v)?),
            _ => return self.err(format!("bad statement {x:?}")),
        })
    }
    fn cell(&self, x: &X) -> R<Cell> {
        Ok(match x { X::Sym(s) if s == "last" => Cell::Last, X::Sym(s) if s == "count" => Cell::Count,
            X::L(l) => match l.as_slice() { [X::Sym(h), n] if h == "votes" => Cell::Votes(self.int(n)? as u8), [X::Sym(h), n] if h == "hist" => Cell::Hist(self.int(n)? as u8), [X::Sym(h), n] if h == "ring" => Cell::Ring(self.int(n)? as u16), _ => return self.err(format!("bad cell {x:?}")) },
            _ => return self.err(format!("bad cell {x:?}")) })
    }
    fn feat(&self, x: &X) -> R<Feat> {
        let field = |s: &X| -> R<F> { match s { X::Sym(n) => FIELD_NAMES.iter().position(|f| f == n).map(|p| FIELDS[p]).map_or_else(|| self.err(format!("no field {n}")), Ok), _ => self.err("field expected".into()) } };
        Ok(match x { X::Sym(s) if s == "prev" => Feat::Prev, X::Sym(_) => Feat::Field(field(x)?),
            X::L(l) => match l.as_slice() { [X::Sym(h), f, n] if h == "/" => Feat::Div(field(f)?, self.int(n)? as i32), [X::Sym(h), f, n] if h == "%" => Feat::Mod(field(f)?, self.int(n)? as i32), [X::Sym(h), n] if h == "since" => Feat::Since(self.int(n)? as i32), _ => return self.err(format!("bad feature {x:?}")) },
            _ => return self.err(format!("bad feature {x:?}")) })
    }
    fn grammar(&self, x: &X) -> R<Grammar> {
        let X::L(l) = x else { return self.err("grammar expected".into()); };
        let mut g = Grammar { feats: vec![], cells: vec![], max_key: 2, backoff: false, cap: 256, per_kb: 0, per_fuel: 0, budget: 1_000_000, gap: 0, paths: false, fast: 0, min: 0 };
        for it in l.iter().skip(1) {
            match it {
                X::Sym(s) if s == "backoff" => g.backoff = true,
                X::Sym(s) if s == "paths" => g.paths = true,
                X::L(v) => match v.as_slice() {
                    [X::Sym(h), rest @ ..] if h == "feats" => g.feats = rest.iter().map(|f| self.feat(f)).collect::<R<_>>()?,
                    [X::Sym(h), rest @ ..] if h == "cells" => g.cells = rest.iter().map(|c| self.cell(c)).collect::<R<_>>()?,
                    [X::Sym(h), n] if h == "keys" => g.max_key = self.int(n)? as u8,
                    [X::Sym(h), n] if h == "cap" => g.cap = self.int(n)? as u16,
                    [X::Sym(h), n] if h == "per-kb" => g.per_kb = self.int(n)? as i32,
                    [X::Sym(h), n] if h == "per-fuel" => g.per_fuel = self.int(n)? as i32,
                    [X::Sym(h), n] if h == "budget" => g.budget = self.int(n)? as i32,
                    [X::Sym(h), n] if h == "gap" => g.gap = self.int(n)? as i32,
                    [X::Sym(h), n] if h == "fast" => g.fast = self.int(n)? as i32,
                    [X::Sym(h), n] if h == "min" => g.min = self.int(n)? as i32,
                    _ => return self.err(format!("bad grammar item {it:?}")),
                },
                _ => return self.err(format!("bad grammar item {it:?}")),
            }
        }
        Ok(g)
    }
    fn a(&self, x: &X) -> R<A> {
        let X::L(l) = x else { return self.err(format!("answer expected, got {x:?}")); };
        Ok(match l.as_slice() {
            [X::Sym(h), s, k, n] if h == "top" => A::Top(self.slot(s)?, self.key(k)?, self.int(n)? as u8),
            [X::Sym(h), e] if h == "val" => A::Val(self.e(e)?),
            [X::Sym(h), rest @ ..] if h == "first" => A::First(rest.iter().map(|a| self.a(a)).collect::<R<_>>()?),
            [X::Sym(h), rest @ ..] if h == "blend" => A::Blend(rest.iter().map(|p| match p { X::L(v) if v.len() == 2 => Ok((self.int(&v[0])? as i32, self.a(&v[1])?)), _ => self.err("(weight answer) expected".into()) }).collect::<R<_>>()?),
            [X::Sym(h), c, a, b] if h == "if" => A::If(self.e(c)?, Box::new(self.a(a)?), Box::new(self.a(b)?)),
            [X::Sym(h), s, k, st, n] if h == "chain" => A::Chain(self.slot(s)?, self.key(k)?, self.e(st)?, self.e(n)?),
            [X::Sym(h), s, a, b, n] if h == "path" => A::Path(self.slot(s)?, self.e(a)?, self.e(b)?, self.int(n)? as u8, None),
            [X::Sym(h), s, a, b, n, X::L(av)] if h == "path" => match av.as_slice() { [X::Sym(k), j, t] if k == "avoid" => A::Path(self.slot(s)?, self.e(a)?, self.e(b)?, self.int(n)? as u8, Some((self.slot(j)?, self.int(t)? as i32))), _ => return self.err("(avoid slot threshold) expected".into()) },
            [X::Sym(h), u, X::Str(v), q] if h == "sub" => A::Sub(self.use_(u)?, v.clone(), self.fq(q)?),
            [X::Sym(h), q, lg, lb, g] if h == "make" => A::Make(self.slot(q)?, self.slot(lg)?, self.slot(lb)?, Box::new(self.grammar(g)?)),
            _ => return self.err(format!("bad answer {x:?}")),
        })
    }
}

/// Parse a description from its text form, and check it.
pub fn parse(src: &str) -> Result<Desc, CheckError> {
    let top = tokens(src)?;
    let [X::L(l)] = top.as_slice() else { return Err(CheckError("parse: one (tool ...) expected".into())); };
    let (name, items) = match l.as_slice() { [X::Sym(h), X::Str(n), rest @ ..] if h == "tool" => (n.clone(), rest), _ => return Err(CheckError("parse: (tool \"name\" ...) expected".into())) };
    let mut slots = vec![];
    let mut names = vec![];
    for it in items {
        if let X::L(v) = it { if let [X::Sym(h), X::Sym(n), X::L(k), cell, X::Sym(vt), cap] = v.as_slice() { if h == "slot" {
            let ty = |s: &X| -> R<Ty> { match s { X::Sym(n) => TY_NAMES.iter().position(|t| t == n).map(|p| TYS[p]).map_or_else(|| Err(CheckError(format!("parse: no type {n}"))), Ok), _ => Err(CheckError("parse: type expected".into())) } };
            let key = k.iter().skip(1).map(ty).collect::<R<Vec<_>>>()?;
            if !matches!(k.first(), Some(X::Sym(s)) if s == "key") { return Err(CheckError("parse: (key ...) expected".into())); }
            let ps = Ps { slots: vec![], uses: vec![] };
            let val = ty(&X::Sym(vt.clone()))?;
            slots.push(Slot { name: n.clone(), key, cell: ps.cell(cell)?, cap: ps.int(cap)? as u16, val });
            names.push(n.clone());
        } } }
    }
    let uses: Vec<String> = items.iter().filter_map(|it| match it { X::L(v) if matches!(v.first(), Some(X::Sym(h)) if h == "uses") => Some(v[1..].iter().filter_map(|u| match u { X::Sym(n) => Some(n.clone()), _ => None }).collect::<Vec<_>>()), _ => None }).flatten().collect();
    let ps = Ps { slots: names, uses: uses.clone() };
    let (mut observe, mut ask, mut forget, mut views) = (vec![], None, vec![], vec![]);
    for it in items {
        let X::L(v) = it else { return ps.err(format!("bad item {it:?}")); };
        match v.as_slice() {
            [X::Sym(h), ..] if h == "slot" || h == "uses" => {}
            [X::Sym(h), X::Sym(n), a] if h == "view" => views.push((n.clone(), ps.a(a)?)),
            [X::Sym(h), rest @ ..] if h == "observe" => observe = rest.iter().map(|s| ps.s(s)).collect::<R<_>>()?,
            [X::Sym(h), rest @ ..] if h == "forget" => forget = rest.iter().map(|s| ps.s(s)).collect::<R<_>>()?,
            [X::Sym(h), a] if h == "ask" => ask = Some(ps.a(a)?),
            _ => return ps.err(format!("bad item {it:?}")),
        }
    }
    let d = Desc { name, slots, observe, ask: ask.ok_or_else(|| CheckError("parse: no (ask ...)".into()))?, forget, uses, views };
    d.check()?;
    Ok(d)
}

// ------------------------------------------------------------------------------- in words

fn e_words(d: &Desc, e: &E) -> String {
    match e {
        E::C(c) if *c == NONE => "nothing".into(), E::C(c) => format!("{c}"), E::F(f) => f.name().into(), E::It => "the place so far".into(),
        E::Op(Op::Div, a, b) => format!("{} in steps of {}", e_words(d, a), e_words(d, b)),
        E::Op(Op::Mod, a, b) => format!("{} in a cycle of {}", e_words(d, a), e_words(d, b)),
        E::Op(Op::Min, a, b) => format!("{} (at most {})", e_words(d, a), e_words(d, b)),
        E::Op(Op::Sub, a, b) => format!("{} less {}", e_words(d, a), e_words(d, b)),
        E::Op(op, a, b) => format!("{} {} {}", e_words(d, a), OP_NAMES[*op as usize], e_words(d, b)),
        E::If(c, a, b) => format!("{} if {}, else {}", e_words(d, a), e_words(d, c), e_words(d, b)),
        E::Sub(u, v, _, n) => format!("what my {} tool{} says (choice {n})", d.uses[*u as usize], if v.is_empty() { String::new() } else { format!("'s {v}") }),
        E::Get(i, k, rd) => { let s = &d.slots[*i as usize].name; let ks: Vec<String> = k.iter().map(|x| e_words(d, x)).collect();
            match rd { Rd::Top(0) => format!("the {s} for {}", ks.join(" and ")), Rd::At => format!("when {s} was last noted for {}", ks.join(" and ")), _ => format!("{s}[{}].{}", ks.join(", "), rd_name(*rd)) } }
    }
}
fn a_words(d: &Desc, a: &A) -> String {
    match a {
        A::Top(i, k, n) => { let s = &d.slots[*i as usize]; let ks: Vec<String> = k.iter().map(|x| e_words(d, x)).collect();
            let what = match s.cell { Cell::Votes(_) | Cell::Hist(_) => format!("the {n} {}s most often noted", TY_NAMES[s.val as usize]), Cell::Last => format!("the last {} noted", TY_NAMES[s.val as usize]), Cell::Count => "the running total".into(), Cell::Ring(_) => "?".into() };
            format!("{what} in {} for this {}", s.name, ks.join(" and ")) }
        A::Val(e) => e_words(d, e),
        A::First(v) => v.iter().map(|x| a_words(d, x)).collect::<Vec<_>>().join("; if none, "),
        A::Blend(v) => format!("a blend of {}", v.iter().map(|(w, x)| format!("{} (weight {w})", a_words(d, x))).collect::<Vec<_>>().join(" and ")),
        A::If(c, x, y) => format!("if {}: {}; otherwise {}", e_words(d, c), a_words(d, x), a_words(d, y)),
        A::Chain(i, _, s, n) => format!("start from {} and follow {} forward {} times", e_words(d, s), d.slots[*i as usize].name, e_words(d, n)),
        A::Path(i, a, b, n, av) => format!("the first step of the shortest way from {} to {} over my map {} (at most {n} steps{})", e_words(d, a), e_words(d, b), d.slots[*i as usize].name, av.map_or(String::new(), |(j, t)| format!(", keeping away from places where {} is {t} or more", d.slots[j as usize].name))),
        A::Sub(u, v, _) => format!("what my {} tool{} says", d.uses[*u as usize], if v.is_empty() { String::new() } else { format!("'s {v}") }),
        A::Make(..) => "a new tool, built from the questions I keep asking and what I have seen".into(),
    }
}
impl Desc {
    /// The description in plain words: what a mind would say if it told someone how it works.
    pub fn explain(&self) -> String {
        let mut o = format!("\"{}\": I keep ", self.name);
        let ss: Vec<String> = self.slots.iter().map(|s| {
            let k: Vec<&str> = s.key.iter().map(|t| TY_NAMES[*t as usize]).collect();
            match s.cell { Cell::Ring(n) => format!("the last {n} things noted ({})", s.name),
                Cell::Votes(n) => format!("for each {}, the {n} {}s most often noted ({})", k.join(" and "), TY_NAMES[s.val as usize], s.name),
                Cell::Last => format!("for each {}, the last {} noted and when ({})", k.join(" and "), TY_NAMES[s.val as usize], s.name),
                Cell::Count => format!("for each {}, a running total ({})", if k.is_empty() { "thing".into() } else { k.join(" and ") }, s.name),
                Cell::Hist(n) => format!("for each {}, how often each of {n} {}s ({})", k.join(" and "), TY_NAMES[s.val as usize], s.name) }
        }).collect();
        o.push_str(&ss.join("; "));
        let _ = write!(o, ". Asked, I give {}.", a_words(self, &self.ask));
        if !self.forget.is_empty() { o.push_str(" Each night it fades."); }
        o
    }
}
