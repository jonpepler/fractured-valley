//! The interpreter: a tool is a description plus its state. Integer only, deterministic, and every
//! call is bounded by **fuel**: each expression node costs 1, each table lookup 1 plus its binary
//! search steps, a new entry a little more, a fade one per entry. A call that runs out stops and
//! says so (`Spent`); its cost never depends on anything but the description, the state and the
//! record, so it is bit-identical everywhere.
//!
//! State is plain sorted vectors (no hash maps): a table is its keys in order and a fixed-size
//! cell per key, so iteration order, eviction and hashing are the same on every machine.

use super::lang::*;
use crate::rng::StateHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

pub type Key = [i32; MAX_KEY];
/// One entry's cell: votes as (value, weight) pairs, last as (value, tick), a count as (sum, n),
/// or histogram bins.
pub type Cells = [i32; 8];

#[derive(Clone, Debug, Default, Hash, PartialEq, Eq)]
pub struct Table { pub keys: Vec<Key>, pub cells: Vec<Cells> }
#[derive(Clone, Debug, Default, Hash, PartialEq, Eq)]
pub struct Ring { pub recs: Vec<Rec>, pub head: usize }
impl Ring {
    /// Records oldest first.
    pub fn iter(&self) -> impl Iterator<Item = &Rec> { let (a, b) = self.recs.split_at(self.head.min(self.recs.len())); b.iter().chain(a.iter()) }
}
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum St { T(Table), R(Ring) }

/// Out of fuel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spent;
pub type Res<T> = Result<T, Spent>;

/// A call's fuel tank, and the other tools of the same mind it may ask (sub-tools, read only:
/// their cost comes out of this tank).
#[derive(Clone, Copy, Debug)]
pub struct Fuel<'a> { pub left: i64, pub used: i64, pub env: &'a [Tool], pub depth: u8 }
/// Sub-tools may call sub-tools this deep.
pub const MAX_DEPTH: u8 = 4;
impl<'a> Fuel<'a> {
    pub fn new(n: i64) -> Fuel<'static> { Fuel { left: n, used: 0, env: &[], depth: 0 } }
    pub fn with(n: i64, env: &'a [Tool]) -> Fuel<'a> { Fuel { left: n, used: 0, env, depth: 0 } }
    #[inline] pub fn spend(&mut self, n: i64) -> Res<()> { if self.left < n { self.used += self.left; self.left = 0; return Err(Spent); } self.left -= n; self.used += n; Ok(()) }
}

/// Ranked (value, score), best first.
pub type Ans = Vec<(i32, i32)>;

/// What an ask gives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Out { Vals(Ans), /// A made tool, and a cheap version of it (for the fast brain) if the grammar asks.
    Made(Box<Desc>, super::maker::Made, Option<Box<(Desc, super::maker::Made)>>) }
impl Out {
    pub fn vals(&self) -> &[(i32, i32)] { match self { Out::Vals(v) => v, _ => &[] } }
    pub fn top(&self) -> i32 { self.vals().first().map_or(NONE, |x| x.0) }
}

/// Measures (not part of the state hash).
#[derive(Clone, Copy, Debug, Default)]
pub struct Use { pub observes: u64, pub asks: u64, pub observe_fuel: u64, pub ask_fuel: u64, pub spent: u64 }

/// A tool: its description and state.
#[derive(Clone, Debug)]
pub struct Tool { pub desc: Arc<Desc>, pub slots: Vec<St>, pub uses: Use }

impl Hash for Tool { fn hash<H: Hasher>(&self, h: &mut H) { h.write_u64(self.desc.digest()); self.slots.hash(h); } }

#[inline] fn probes(n: usize) -> i64 { 1 + (usize::BITS - n.leading_zeros()) as i64 }
#[inline] fn none(x: i32) -> bool { x == NONE }

fn arith(op: Op, a: i32, b: i32) -> i32 {
    if none(a) || none(b) { return match op { Op::Eq => (a == b) as i32, Op::Ne => (a != b) as i32, Op::And => 0, Op::Or => (!none(a) && a != 0 || !none(b) && b != 0) as i32, _ => NONE }; }
    let (x, y) = (a as i64, b as i64);
    let r = match op {
        Op::Add => x + y, Op::Sub => x - y, Op::Mul => x * y,
        Op::Div => if y == 0 { return NONE } else { x.div_euclid(y) },
        Op::Mod => if y == 0 { return NONE } else { x.rem_euclid(y) },
        Op::Min => x.min(y), Op::Max => x.max(y), Op::Eq => (x == y) as i64, Op::Ne => (x != y) as i64,
        Op::Lt => (x < y) as i64, Op::Le => (x <= y) as i64, Op::And => (x != 0 && y != 0) as i64, Op::Or => (x != 0 || y != 0) as i64,
    };
    // Saturate, never wrap; NONE (i32::MIN) is kept out of reach.
    r.clamp(i32::MIN as i64 + 1, i32::MAX as i64) as i32
}

fn empty_cells(c: Cell) -> Cells { match c { Cell::Votes(_) => [NONE, 0, NONE, 0, NONE, 0, NONE, 0], Cell::Last => [NONE, NONE, 0, 0, 0, 0, 0, 0], _ => [0; 8] } }
/// How many i32s a cell really needs (the packed size).
pub fn cell_ints(c: Cell) -> usize { match c { Cell::Votes(k) => 2 * k as usize, Cell::Last | Cell::Count => 2, Cell::Hist(n) => n as usize, Cell::Ring(_) => 0 } }
fn strength(c: Cell, x: &Cells) -> i64 {
    match c { Cell::Votes(_) => (0..4).map(|j| (x[2 * j + 1] as i64).abs()).sum(), Cell::Last => x[1] as i64, Cell::Count => (x[0] as i64).abs(), Cell::Hist(n) => x[..n as usize].iter().map(|&v| v as i64).sum(), Cell::Ring(_) => 0 }
}
fn fade_w(w: i32, sh: u8) -> i32 { let n = w - (w >> sh); if n == w && w != 0 { w - w.signum() } else { n } }

impl Tool {
    pub fn new(desc: Arc<Desc>) -> Tool {
        let slots = desc.slots.iter().map(|s| match s.cell { Cell::Ring(_) => St::R(Ring::default()), _ => St::T(Table::default()) }).collect();
        Tool { desc, slots, uses: Use::default() }
    }
    pub fn from(d: Desc) -> Tool { Tool::new(Arc::new(d)) }
    pub fn state_hash(&self) -> u64 { let mut h = StateHasher::new(0x7003); self.hash(&mut h); h.finish() }

    /// Packed bytes of state: per entry, its key and the cell's real size; per ring record, the 14 fields.
    pub fn bytes(&self) -> usize {
        self.desc.slots.iter().zip(&self.slots).map(|(s, st)| match st { St::T(t) => t.keys.len() * 4 * (s.key.len() + cell_ints(s.cell)), St::R(r) => r.recs.len() * 4 * NF }).sum()
    }
    pub fn entries(&self) -> usize { self.slots.iter().map(|s| match s { St::T(t) => t.keys.len(), St::R(r) => r.recs.len() }).sum() }
    pub fn table(&self, i: usize) -> Option<&Table> { match self.slots.get(i) { Some(St::T(t)) => Some(t), _ => None } }
    pub fn ring(&self, i: usize) -> Option<&Ring> { match self.slots.get(i) { Some(St::R(r)) => Some(r), _ => None } }

    fn key(&self, k: &[E], r: &Rec, it: i32, f: &mut Fuel) -> Res<Option<Key>> {
        let mut key = [NONE; MAX_KEY];
        for (j, e) in k.iter().enumerate() { let v = self.eval(e, r, it, f)?; if none(v) { return Ok(None); } key[j] = v; }
        Ok(Some(key))
    }
    fn find(&self, i: u8, key: &Key, f: &mut Fuel) -> Res<Option<&Cells>> {
        let St::T(t) = &self.slots[i as usize] else { return Ok(None) };
        f.spend(probes(t.keys.len()))?;
        Ok(t.keys.binary_search(key).ok().map(|p| &t.cells[p]))
    }

    /// Evaluate an expression on a record.
    pub fn eval(&self, e: &E, r: &Rec, it: i32, f: &mut Fuel) -> Res<i32> {
        f.spend(1)?;
        Ok(match e {
            E::C(c) => *c,
            E::F(x) => r[*x as usize],
            E::It => it,
            E::Op(op, a, b) => { let x = self.eval(a, r, it, f)?; if *op == Op::And && x == 0 { return Ok(0); } arith(*op, x, self.eval(b, r, it, f)?) }
            E::If(c, a, b) => { let c = self.eval(c, r, it, f)?; if !none(c) && c != 0 { self.eval(a, r, it, f)? } else { self.eval(b, r, it, f)? } }
            E::Sub(u, v, q, n) => { let a = self.sub(*u, v, q, r, it, f)?; a.get(*n as usize).map_or(NONE, |x| x.0) }
            E::Get(i, k, rd) => {
                let Some(key) = self.key(k, r, it, f)? else { return Ok(if *rd == Rd::Has { 0 } else { NONE }) };
                let cell = self.desc.slots[*i as usize].cell;
                match self.find(*i, &key, f)? {
                    None => if *rd == Rd::Has { 0 } else if *rd == Rd::Count { 0 } else { NONE },
                    Some(x) => read(cell, x, *rd),
                }
            }
        })
    }

    /// Ask a sub-tool (found by name among the tools in the fuel's environment).
    fn sub(&self, u: u8, view: &str, q: &[(F, E)], r: &Rec, it: i32, f: &mut Fuel) -> Res<Ans> {
        f.spend(2)?;
        let Some(name) = self.desc.uses.get(u as usize) else { return Ok(vec![]) };
        let env = f.env;
        let Some(t) = env.iter().find(|t| &t.desc.name == name) else { return Ok(vec![]) };
        if f.depth >= MAX_DEPTH { return Ok(vec![]); }
        let mut qr = rec();
        for (fl, e) in q { qr[*fl as usize] = self.eval(e, r, it, f)?; }
        let a = if view.is_empty() { &t.desc.ask } else { match t.desc.views.iter().find(|x| x.0 == view) { Some(x) => &x.1, None => return Ok(vec![]) } };
        if matches!(a, A::Make(..)) { return Ok(vec![]); }
        f.depth += 1;
        let out = t.answer(a, &qr, f);
        f.depth -= 1;
        out
    }
    /// Ask through a named view ("" for the main answer), read only, with sub-tools from `env`.
    pub fn query(&self, view: &str, q: &Rec, limit: i64, env: &[Tool]) -> (Res<Ans>, i64) {
        let mut f = Fuel::with(limit, env);
        let a = if view.is_empty() { Some(&self.desc.ask) } else { self.desc.views.iter().find(|x| x.0 == view).map(|x| &x.1) };
        let r = match a { Some(A::Make(..)) | None => Ok(vec![]), Some(a) => self.answer(a, q, &mut f) };
        (r, f.used)
    }

    fn run(&mut self, s: &S, r: &Rec, f: &mut Fuel) -> Res<()> {
        f.spend(1)?;
        match s {
            S::If(c, b) => { let c = self.eval(c, r, NONE, f)?; if !none(c) && c != 0 { for x in b { self.run(x, r, f)?; } } }
            S::Put(i, k, v, w) => {
                let Some(key) = self.key(k, r, NONE, f)? else { return Ok(()) };
                let (v, w) = (self.eval(v, r, NONE, f)?, self.eval(w, r, NONE, f)?);
                let slot = &self.desc.slots[*i as usize];
                let (cell, cap) = (slot.cell, slot.cap as usize);
                if none(w) || (none(v) && !matches!(cell, Cell::Count)) { return Ok(()); }
                let at = r[F::Tick as usize];
                let St::T(t) = &mut self.slots[*i as usize] else { return Ok(()) };
                f.spend(probes(t.keys.len()))?;
                let p = match t.keys.binary_search(&key) {
                    Ok(p) => p,
                    Err(mut p) => {
                        // A new entry: shifting costs a little; a full table first evicts its weakest.
                        if t.keys.len() >= cap {
                            f.spend(t.keys.len() as i64 / 4 + 1)?;
                            let mut wk = 0; let mut ws = i64::MAX;
                            for (j, c) in t.cells.iter().enumerate() { let s = strength(cell, c); if s < ws { ws = s; wk = j; } }
                            t.keys.remove(wk); t.cells.remove(wk);
                            if wk < p { p -= 1; }
                        }
                        f.spend(t.keys.len() as i64 / 32 + 2)?;
                        t.keys.insert(p, key); t.cells.insert(p, empty_cells(cell));
                        p
                    }
                };
                put(cell, &mut t.cells[p], v, w, at);
            }
            S::Push(i) => {
                let Cell::Ring(len) = self.desc.slots[*i as usize].cell else { return Ok(()) };
                let St::R(q) = &mut self.slots[*i as usize] else { return Ok(()) };
                f.spend(2)?;
                if q.recs.len() < len as usize { q.recs.push(*r); } else { let h = q.head; q.recs[h] = *r; q.head = (h + 1) % len as usize; }
            }
            S::Fade(i, sh) => {
                let cell = self.desc.slots[*i as usize].cell;
                let St::T(t) = &mut self.slots[*i as usize] else { return Ok(()) };
                f.spend(t.keys.len() as i64 + 1)?;
                for c in t.cells.iter_mut() { fade(cell, c, *sh); }
                let mut j = 0;
                while j < t.keys.len() { if strength(cell, &t.cells[j]) == 0 && !matches!(cell, Cell::Last) { t.keys.remove(j); t.cells.remove(j); } else { j += 1; } }
            }
        }
        Ok(())
    }

    /// Tell the tool a record. At most `limit` fuel.
    pub fn observe(&mut self, r: &Rec, limit: i64) -> Res<i64> {
        let mut f = Fuel::new(limit);
        let d = self.desc.clone();
        let mut out = Ok(());
        for s in &d.observe { if let Err(e) = self.run(s, r, &mut f) { out = Err(e); break; } }
        self.uses.observes += 1; self.uses.observe_fuel += f.used as u64;
        if out.is_err() { self.uses.spent += 1; }
        out.map(|_| f.used)
    }
    /// Run the forget block (sleep). At most `limit` fuel.
    pub fn forget(&mut self, now: i32, limit: i64) -> Res<i64> {
        let mut f = Fuel::new(limit);
        let d = self.desc.clone();
        let r = rec_of(&[(F::Tick, now)]);
        for s in &d.forget { self.run(s, &r, &mut f)?; }
        Ok(f.used)
    }
    /// Ask the tool. At most `limit` fuel.
    pub fn ask(&mut self, q: &Rec, limit: i64) -> (Res<Out>, i64) {
        let mut f = Fuel::new(limit);
        let d = self.desc.clone();
        let out = match &d.ask { A::Make(qs, l, b, g) => { f.left = f.left.min(g.budget as i64); super::maker::make(self, *qs, *l, *b, g, &mut f).map(|m| match m { Some((d, s, fast)) => Out::Made(Box::new(d), s, fast.map(Box::new)), None => Out::Vals(vec![]) }) }
            a => self.answer(a, q, &mut f).map(Out::Vals) };
        self.uses.asks += 1; self.uses.ask_fuel += f.used as u64;
        if out.is_err() { self.uses.spent += 1; }
        (out, f.used)
    }
    /// Ask, read-only (no measures kept): for scoring candidates.
    pub fn answer_only(&self, q: &Rec, f: &mut Fuel) -> Res<Ans> { self.answer(&self.desc.ask, q, f) }

    pub fn answer(&self, a: &A, q: &Rec, f: &mut Fuel) -> Res<Ans> {
        f.spend(1)?;
        Ok(match a {
            A::Top(i, k, n) => {
                let Some(key) = self.key(k, q, NONE, f)? else { return Ok(vec![]) };
                let cell = self.desc.slots[*i as usize].cell;
                match self.find(*i, &key, f)? { None => vec![], Some(x) => { f.spend(*n as i64)?; ranked(cell, x, *n as usize) } }
            }
            A::Val(e) => { let v = self.eval(e, q, NONE, f)?; if none(v) { vec![] } else { vec![(v, 1000)] } }
            A::First(v) => { for x in v { let r = self.answer(x, q, f)?; if !r.is_empty() { return Ok(r); } } vec![] }
            A::Blend(v) => {
                let mut acc: Vec<(i32, i64)> = vec![];
                for (w, x) in v {
                    let r = self.answer(x, q, f)?;
                    let tot: i64 = r.iter().map(|p| p.1.max(0) as i64).sum();
                    if tot == 0 { continue; }
                    for (val, s) in r { let add = s.max(0) as i64 * *w as i64 / tot; f.spend(1)?; match acc.iter_mut().find(|p| p.0 == val) { Some(p) => p.1 += add, None => acc.push((val, add)) } }
                }
                acc.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
                acc.into_iter().take(8).map(|(v, s)| (v, s.clamp(i32::MIN as i64 + 1, i32::MAX as i64) as i32)).collect()
            }
            A::If(c, x, y) => { let c = self.eval(c, q, NONE, f)?; if !none(c) && c != 0 { self.answer(x, q, f)? } else { self.answer(y, q, f)? } }
            A::Chain(i, k, s, n) => {
                let mut it = self.eval(s, q, NONE, f)?;
                if none(it) { return Ok(vec![]); }
                let steps = self.eval(n, q, NONE, f)?;
                let steps = if none(steps) { 0 } else { steps.clamp(0, 16) };
                let cell = self.desc.slots[*i as usize].cell;
                for _ in 0..steps {
                    let Some(key) = self.key(k, q, it, f)? else { break };
                    match self.find(*i, &key, f)?.map(|x| read(cell, x, Rd::Top(0))) { Some(v) if !none(v) => it = v, _ => break }
                }
                vec![(it, 1000)]
            }
            A::Path(i, a, b, n, av) => {
                let (from, to) = (self.eval(a, q, NONE, f)?, self.eval(b, q, NONE, f)?);
                if none(from) || none(to) || from == to { return Ok(vec![]); }
                // Breadth first over the map's neighbours (best vote first), at most 64 places.
                let cell = self.desc.slots[*i as usize].cell;
                let mut seen: Vec<(i32, i32)> = vec![(from, NONE)]; // (place, first step)
                let mut head = 0;
                let mut depth_end = 1; let mut depth = 0;
                while head < seen.len() && depth < *n as i32 && seen.len() < 64 {
                    let (u, first) = seen[head]; head += 1;
                    if let Some(x) = self.find(*i, &[u, NONE, NONE, NONE], f)? {
                        let nb = ranked(cell, x, 4);
                        f.spend(nb.len() as i64)?;
                        for (v, _) in nb {
                            if seen.iter().any(|p| p.0 == v) { continue; }
                            if v != to { if let Some((j, t)) = av { if let Some(c) = self.find(*j, &[v, NONE, NONE, NONE], f)? { if read(self.desc.slots[*j as usize].cell, c, Rd::Count) >= *t { continue; } } } }
                            let fs = if none(first) { v } else { first };
                            if v == to { return Ok(vec![(fs, 1000 - 10 * (depth + 1))]); }
                            seen.push((v, fs));
                        }
                    }
                    if head == depth_end { depth += 1; depth_end = seen.len(); }
                }
                vec![]
            }
            A::Sub(u, v, qq) => self.sub(*u, v, qq, q, NONE, f)?,
            A::Make(..) => vec![],
        })
    }
}

fn read(c: Cell, x: &Cells, rd: Rd) -> i32 {
    match (c, rd) {
        (_, Rd::Has) => 1,
        (Cell::Votes(_), Rd::Top(n)) => if (n as usize) < 4 && x[2 * n as usize + 1] > 0 { x[2 * n as usize] } else { NONE },
        (Cell::Votes(_), Rd::W(n)) => if (n as usize) < 4 { x[2 * n as usize + 1] } else { 0 },
        (Cell::Votes(_), Rd::Count) => (0..4).map(|j| x[2 * j + 1].max(0)).sum(),
        (Cell::Last, Rd::Top(0)) => x[0],
        (Cell::Last, Rd::At) => x[1],
        (Cell::Count, Rd::Count) | (Cell::Count, Rd::Top(0)) => x[0],
        (Cell::Count, Rd::W(_)) => x[1],
        (Cell::Hist(m), Rd::Top(n)) => { let r = ranked(c, x, m as usize); r.get(n as usize).map_or(NONE, |p| p.0) }
        (Cell::Hist(m), Rd::Count) => x[..m as usize].iter().sum(),
        (Cell::Hist(m), Rd::W(n)) => { let r = ranked(c, x, m as usize); r.get(n as usize).map_or(0, |p| p.1) }
        _ => NONE,
    }
}
fn ranked(c: Cell, x: &Cells, n: usize) -> Ans {
    match c {
        Cell::Votes(_) => (0..4).filter(|&j| x[2 * j + 1] > 0 && !none(x[2 * j])).map(|j| (x[2 * j], x[2 * j + 1])).take(n).collect(),
        Cell::Last => if none(x[0]) { vec![] } else { vec![(x[0], 1000)] },
        Cell::Count => vec![(x[0], x[1])],
        Cell::Hist(m) => { let mut v: Ans = (0..m as usize).filter(|&j| x[j] > 0).map(|j| (j as i32, x[j])).collect(); v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0))); v.truncate(n); v }
        Cell::Ring(_) => vec![],
    }
}
fn put(c: Cell, x: &mut Cells, v: i32, w: i32, at: i32) {
    match c {
        Cell::Votes(k) => {
            let k = k as usize;
            if let Some(j) = (0..k).find(|&j| x[2 * j] == v) { x[2 * j + 1] = arith(Op::Add, x[2 * j + 1], w); }
            else if let Some(j) = (0..k).find(|&j| none(x[2 * j])) { x[2 * j] = v; x[2 * j + 1] = w; }
            else {
                // Space-saving (Metwally, Agrawal and El Abbadi, ICDT 2005): the weakest gives up its place,
                // and the newcomer inherits its weight.
                let j = (0..k).min_by_key(|&j| (x[2 * j + 1], std::cmp::Reverse(j))).unwrap();
                // (A negative weight, counter-evidence, is not inherited.)
                x[2 * j] = v; x[2 * j + 1] = arith(Op::Add, x[2 * j + 1].max(0), w);
            }
            // Keep ranked: weight down, value up (insertion sort of ≤ 4).
            for a in 1..k { let mut b = a; while b > 0 { let (p, q) = ((x[2 * b - 2], x[2 * b - 1]), (x[2 * b], x[2 * b + 1]));
                let worse = |p: (i32, i32), q: (i32, i32)| none(p.0) && !none(q.0) || !none(p.0) && !none(q.0) && (q.1 > p.1 || q.1 == p.1 && q.0 < p.0);
                if worse(p, q) { x.swap(2 * b - 2, 2 * b); x.swap(2 * b - 1, 2 * b + 1); b -= 1; } else { break; } } }
        }
        Cell::Last => { x[0] = v; x[1] = at; }
        Cell::Count => { x[0] = arith(Op::Add, x[0], w); x[1] = arith(Op::Add, x[1], 1); }
        Cell::Hist(n) => { if !none(v) { let j = v.clamp(0, n as i32 - 1) as usize; x[j] = arith(Op::Add, x[j], w); } }
        Cell::Ring(_) => {}
    }
}
fn fade(c: Cell, x: &mut Cells, sh: u8) {
    match c {
        Cell::Votes(k) => { for j in 0..k as usize { x[2 * j + 1] = fade_w(x[2 * j + 1], sh); if x[2 * j + 1] == 0 { x[2 * j] = NONE; } }
            // re-rank (zeros to the end)
            let mut v: Vec<(i32, i32)> = (0..4).map(|j| (x[2 * j], x[2 * j + 1])).collect();
            v.sort_by(|a, b| none(a.0).cmp(&none(b.0)).then(b.1.cmp(&a.1)).then(a.0.cmp(&b.0)));
            for (j, p) in v.into_iter().enumerate() { x[2 * j] = p.0; x[2 * j + 1] = p.1; } }
        Cell::Count => x[0] = fade_w(x[0], sh),
        Cell::Hist(n) => { for j in 0..n as usize { x[j] = fade_w(x[j], sh); } }
        _ => {}
    }
}
