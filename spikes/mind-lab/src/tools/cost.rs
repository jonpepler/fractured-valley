//! Static cost: the most fuel one call can use, worked out from the description alone, without
//! running it. It is possible by construction: every table has a known cap, every loop a fixed
//! bound (`chain` 16 steps, `path` 64 places, answers 8 values), and a sub-tool is costed from
//! its own description, depth-limited as the interpreter is.
//!
//! The bounds mirror the interpreter's fuel charges one for one (`run.rs`); `tests/tools_cost.rs`
//! checks that the bound always covers the measured cost.
//!
//! **Reach** (the owner, after 8.4): which tools a mind may use at a moment depends on its
//! state. The fast brain that runs every tick has a small ceiling, the slow brain a larger one,
//! idle time and sleep none; stress cuts the ceiling as it cuts how far thought spreads.

use super::lang::*;
use super::run::MAX_DEPTH;

#[inline] fn probes(n: usize) -> i64 { 1 + (usize::BITS - n.leading_zeros()) as i64 }
/// Most values an answer can hold.
const MAX_ANS: i64 = 8;
/// Most places a path search holds.
const PATH_PLACES: i64 = 64;

/// The descriptions a sub-tool call may resolve to (the mind's other tools), by name.
pub type Env<'a> = &'a [&'a Desc];

fn cap(d: &Desc, i: u8) -> usize { d.slots.get(i as usize).map_or(0, |s| s.cap as usize) }

/// Worst-case fuel of evaluating an expression.
pub fn e_cost(d: &Desc, e: &E, env: Env, depth: u8) -> i64 {
    1 + match e {
        E::C(_) | E::F(_) | E::It => 0,
        E::Op(_, a, b) => e_cost(d, a, env, depth) + e_cost(d, b, env, depth),
        E::If(c, a, b) => e_cost(d, c, env, depth) + e_cost(d, a, env, depth).max(e_cost(d, b, env, depth)),
        E::Get(i, k, _) => k.iter().map(|x| e_cost(d, x, env, depth)).sum::<i64>() + probes(cap(d, *i)),
        E::Sub(u, v, q, _) => sub_cost(d, *u, v, q, env, depth),
    }
}

fn sub_cost(d: &Desc, u: u8, view: &str, q: &[(F, E)], env: Env, depth: u8) -> i64 {
    let Some(name) = d.uses.get(u as usize) else { return 2 };
    let Some(t) = env.iter().find(|t| &t.name == name) else { return 2 };
    if depth >= MAX_DEPTH { return 2; }
    let a = if view.is_empty() { Some(&t.ask) } else { t.views.iter().find(|x| x.0 == view).map(|x| &x.1) };
    let qc: i64 = q.iter().map(|(_, e)| e_cost(d, e, env, depth)).sum();
    match a { Some(A::Make(..)) | None => 2 + qc, Some(a) => 2 + qc + a_cost(t, a, env, depth + 1) }
}

/// Worst-case fuel of an answer.
pub fn a_cost(d: &Desc, a: &A, env: Env, depth: u8) -> i64 {
    let keys = |k: &[E]| k.iter().map(|x| e_cost(d, x, env, depth)).sum::<i64>();
    1 + match a {
        A::Top(i, k, n) => keys(k) + probes(cap(d, *i)) + *n as i64,
        A::Val(e) => e_cost(d, e, env, depth),
        A::First(v) => v.iter().map(|x| a_cost(d, x, env, depth)).sum(),
        A::Blend(v) => v.iter().map(|(_, x)| a_cost(d, x, env, depth) + MAX_ANS).sum(),
        A::If(c, x, y) => e_cost(d, c, env, depth) + a_cost(d, x, env, depth).max(a_cost(d, y, env, depth)),
        A::Chain(i, k, s, n) => e_cost(d, s, env, depth) + e_cost(d, n, env, depth) + 16 * (keys(k) + probes(cap(d, *i))),
        A::Path(i, a, b, _, av) => {
            let avoid = av.map_or(0, |(j, _)| probes(cap(d, j)));
            e_cost(d, a, env, depth) + e_cost(d, b, env, depth) + PATH_PLACES * (probes(cap(d, *i)) + 4 + 4 * avoid)
        }
        A::Sub(u, v, q) => sub_cost(d, *u, v, q, env, depth),
        A::Make(_, _, _, g) => g.budget as i64,
    }
}

fn s_cost(d: &Desc, s: &S) -> i64 {
    let none: Env = &[];
    1 + match s {
        S::If(c, b) => e_cost(d, c, none, 0) + b.iter().map(|x| s_cost(d, x)).sum::<i64>(),
        S::Put(i, k, v, w) => { let c = cap(d, *i); k.iter().map(|x| e_cost(d, x, none, 0)).sum::<i64>() + e_cost(d, v, none, 0) + e_cost(d, w, none, 0) + probes(c) + (c as i64 / 4 + 1) + (c as i64 / 32 + 2) }
        S::Push(_) => 2,
        S::Fade(i, _) => cap(d, *i) as i64 + 1,
        S::Drop(i, k, v) => k.iter().map(|x| e_cost(d, x, none, 0)).sum::<i64>() + e_cost(d, v, none, 0) + probes(cap(d, *i)) + 4,
    }
}

/// The static bounds of a tool's calls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Bound { pub ask: i64, pub observe: i64, pub forget: i64 }

/// The bound of a description's ask (its main answer), observe and forget, with sub-tools
/// resolved in `env`.
pub fn bound(d: &Desc, env: Env) -> Bound {
    Bound { ask: a_cost(d, &d.ask, env, 0), observe: d.observe.iter().map(|s| s_cost(d, s)).sum(), forget: d.forget.iter().map(|s| s_cost(d, s)).sum() }
}
/// The bound of one named view ("" for the main answer).
pub fn view_bound(d: &Desc, view: &str, env: Env) -> i64 {
    if view.is_empty() { return a_cost(d, &d.ask, env, 0); }
    d.views.iter().find(|x| x.0 == view).map_or(1, |x| a_cost(d, &x.1, env, 0))
}

/// Which brain is asking.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum Brain {
    /// The reflex and procedure layer that runs every tick.
    Fast,
    /// Thought (a think).
    Slow,
    /// Idle time and sleep (consolidation).
    Idle,
}

/// Ceilings on a call's static bound, by brain (fuel).
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct Ceilings { pub fast: i64, pub slow: i64, pub idle: i64 }
impl Default for Ceilings { fn default() -> Self { Ceilings { fast: 64, slow: 4_000, idle: i64::MAX / 4 } } }

/// The ceiling for this brain under this stress (0..1000). Stress cuts it as it cuts spreading:
/// by `stress × cut / 1000` per mille, where `cut` is the temperament's own (8.4,
/// `Temperament::stress_cut`, 250 + reactivity × 0.7). Idle time is not cut.
pub fn ceiling(c: &Ceilings, brain: Brain, stress: i32, cut: i32) -> i64 {
    let base = match brain { Brain::Fast => c.fast, Brain::Slow => c.slow, Brain::Idle => return c.idle };
    let keep = 1000 - (stress.clamp(0, 1000) as i64 * cut.clamp(0, 1000) as i64 / 1000);
    base * keep / 1000
}
