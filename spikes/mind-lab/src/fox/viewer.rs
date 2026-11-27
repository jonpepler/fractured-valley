//! Writes a recorded scenario as one self-contained HTML page (inline JS and SVG): the graph with
//! activation spreading hop by hop, the fox on the map, modulator bars. Ported from
//! `spikes/thought-graph` (`viewer.rs`, same template). The page is drawn from the mind's
//! **expanded** form: the runtime form is expanded once, at the end of the recording, and fast
//! weights and flows are mapped from link keys to the expanded graph's link indices.

use super::sim::Sim;
use super::world::{DogMode, GRID_H, GRID_W};
use crate::graph::{link_pos, link_src, lt, tag, LinkKey};
use crate::mind::Action;
use crate::store::{Expanded, Store};
use crate::think::Form;
use std::fmt::Write;

const TEMPLATE: &str = include_str!("../../viewer/template.html");

fn esc(s: &str) -> String { s.replace('\\', "\\\\").replace('"', "\\\"") }

fn action_text<S: Store>(a: &Action, sim: &Sim<S>) -> String {
    match *a {
        Action::Stay => "stay".into(),
        Action::WalkTo { x, y } => match sim.m.place_nodes.iter().find(|&&p| sim.m.store.anchor(p).map(|a| a.xy()) == Some((x, y))) {
            Some(&p) => format!("walk to {}", sim.m.name(p)),
            None => format!("walk toward ({x}, {y})"),
        },
        Action::Flee { from_x, from_y, speed } => format!("flee from ({from_x}, {from_y}), {speed} cells a tick"),
        Action::Snap { .. } => "snap at the dog".into(),
        Action::Eat { entity } => format!("eat {}", sim.w.entity(entity).map(|e| e.name.as_str()).unwrap_or("?")),
        Action::Sleep => "sleep".into(),
        Action::Hold { .. } => "hold its ground".into(),
        Action::Freeze => "freeze".into(),
        Action::Call => "call for help".into(),
        Action::Charge { .. } => "charge".into(),
    }
}

/// A key moment to export as a picture: (tick, hop or None for the end of the tick, caption).
pub type Key = (u32, Option<usize>, String);

/// The page for a recorded scenario.
pub fn page<S: Store>(sim: &Sim<S>, title: &str, subtitle: &str, keys: &[Key]) -> String {
    let m = &sim.m;
    let ex: Expanded = { let r = m.store.clone(); expand_any(&r) };
    let li = |k: LinkKey| ex.offsets[link_src(k) as usize] as usize + link_pos(k);
    let mut j = String::with_capacity(1 << 20);
    write!(j, "{{\"title\":\"{}\",\"subtitle\":\"{}\",\"form\":\"runtime (shared base + overlay), drawn expanded\",\"grid\":[{},{}],", esc(title), esc(subtitle), GRID_W, GRID_H).unwrap();
    j.push_str("\"places\":[");
    let home = m.home();
    let mut first_p = true;
    for (pi, &pn) in m.place_nodes.iter().enumerate() {
        if !m.place_learnt(pi) { continue; }
        if !first_p { j.push(',') } first_p = false;
        let (x, y) = ex.anchor(pn).unwrap().xy();
        write!(j, "{{\"name\":\"{}\",\"x\":{},\"y\":{},\"home\":{}}}", esc(&ex.names[pn as usize]), x, y, pn == home).unwrap();
    }
    j.push_str("],\"nodes\":[");
    for n in 0..ex.len() as u32 {
        if n > 0 { j.push(',') }
        let t = ex.tags[n as usize];
        let unlearnt = m.place_nodes.iter().position(|&p| p == n).is_some_and(|pi| !m.place_learnt(pi));
        let kind = if unlearnt { "concept" } else if t & tag::PLACE != 0 { "place" } else if t & tag::ANCHORED != 0 { "thing" } else if t & tag::BODY != 0 { "body" } else { "concept" };
        let (x, y) = ex.anchor(n).map(|a| a.xy()).unwrap_or((-1, -1));
        let home_place = m.place_nodes.iter().position(|&p| p == n).map(|i| i as i32).unwrap_or(-1);
        let at = ex.range(n).find(|&l| ex.ltype[l] == lt::AT).map(|l| ex.targets[l] as i32).unwrap_or(-1);
        write!(j, "{{\"name\":\"{}\",\"kind\":\"{}\",\"x\":{},\"y\":{},\"at\":{},\"place\":{}}}", esc(&ex.names[n as usize]), kind, x, y, at, home_place).unwrap();
    }
    j.push_str("],\"links\":[");
    let mut first = true;
    for u in 0..ex.len() as u32 {
        for l in ex.range(u) {
            if !first { j.push(',') } first = false;
            write!(j, "[{},{},{},{}]", u, ex.targets[l], ex.ltype[l], ex.slow[l]).unwrap();
        }
    }
    j.push_str("],\"linkTypes\":[");
    for (i, n) in lt::NAMES.iter().enumerate() { if i > 0 { j.push(',') } write!(j, "\"{n}\"").unwrap(); }
    j.push_str("],\"snapshots\":[");
    for (i, (t, w, v)) in sim.snapshots.iter().enumerate() {
        if i > 0 { j.push(',') }
        // Nodes and links learnt later have no value in an earlier snapshot: pad with zeros.
        let mut w = w.clone(); w.resize(ex.slow.len(), 0);
        let mut v = v.clone(); v.resize(ex.val.len(), 0);
        write!(j, "{{\"t\":{t},\"w\":{:?},\"val\":{:?}}}", w, v).unwrap();
    }
    j.push_str("],\"frames\":[");
    for (i, f) in sim.frames.iter().enumerate() {
        if i > 0 { j.push(',') }
        let dog = match f.dog {
            Some((x, y, mode)) => format!("[{x},{y},\"{}\"]", match mode { DogMode::Guard => "guards", DogMode::Chase { .. } => "chases", DogMode::Return => "returns" }),
            None => "null".into(),
        };
        let fires: Vec<String> = f.fires.iter().map(|&(x, y, e)| format!("[{x},{y},{}]", e as u8)).collect();
        let burrows: Vec<String> = f.burrows.iter().map(|&(x, y)| format!("[{x},{y}]")).collect();
        let hops: Vec<String> = f.hops.iter().map(|h| { let fl: Vec<String> = h.flows.iter().map(|&(u, v, _, a)| format!("[{u},{v},{a}]")).collect(); format!("[{}]", fl.join(",")) }).collect();
        let fast: Vec<String> = f.fast.iter().filter(|&&(k, _)| li(k) < ex.targets.len()).map(|&(k, w)| format!("[{},{w}]", li(k))).collect();
        let per: Vec<String> = f.percepts.iter().map(|&(n, e)| format!("[{n},{e}]")).collect();
        let ev: Vec<String> = f.events.iter().map(|e| format!("\"{}\"", esc(e))).collect();
        let mut act = vec![0i32; ex.len()];
        for &(n, a) in &f.act { if (n as usize) < act.len() { act[n as usize] = a; } }
        write!(j, "{{\"t\":{},\"fox\":[{},{}],\"dog\":{},\"fires\":[{}],\"m\":[{},{},{},{},{},{}],\"act\":{:?},\"hops\":[{}],\"ws\":{:?},\"action\":\"{}\",\"why\":\"{}\",\"ev\":[{}],\"fast\":[{}],\"per\":[{}],\"slept\":{},\"burrows\":[{}],\"backlog\":{}}}",
            f.tick, f.fox.0, f.fox.1, dog, fires.join(","), f.mods.hunger, f.mods.arousal, f.mods.mood, f.mods.fatigue, f.pain, f.mods.stress,
            act, hops.join(","), f.ws, esc(&action_text(&f.action, sim)), esc(&f.why), ev.join(","), fast.join(","), per.join(","), f.slept, burrows.join(","), f.backlog).unwrap();
    }
    j.push_str("],\"keys\":[");
    for (i, (t, h, c)) in keys.iter().enumerate() {
        if i > 0 { j.push(',') }
        write!(j, "{{\"t\":{t},\"h\":{},\"label\":\"{}\"}}", h.map(|h| h as i64).unwrap_or(-1), esc(c)).unwrap();
    }
    write!(j, "],\"persist\":{},\"hopsPerTick\":{}}}", m.params.persist, m.params.hops).unwrap();
    TEMPLATE.replace("/*TITLE*/", title).replace("/*DATA*/", &j)
}

/// The expanded form of any store, read through the `Store` interface (so it works for either form).
pub fn expand_any<S: Store>(s: &S) -> Expanded {
    let mut e = Expanded { nbase: s.len(), offsets: vec![0], ..Default::default() };
    let mut ls = vec![];
    for u in 0..s.len() as u32 {
        e.val.push(s.val(u) as i16); e.tags.push(s.tags(u)); e.keys.push(s.key(u)); e.names.push(s.name(u).to_string());
        if let Some(a) = s.anchor(u) { e.anchors.push((u, a)); }
        ls.clear(); s.links(u, &mut ls);
        e.base_deg.push(ls.len() as u32);
        for l in &ls { e.targets.push(l.to); e.ltype.push(l.ty); e.slow.push(l.slow); e.lval.push(l.lval); e.fast.push(l.fast); }
        e.offsets.push(e.targets.len() as u32);
    }
    e
}

/// Slow weights and node valences in expanded order (for the viewer's snapshots).
pub fn snapshot<S: Store>(s: &S) -> (Vec<i16>, Vec<i16>) {
    (s.slow_all(), (0..s.len() as u32).map(|n| s.val(n) as i16).collect())
}
