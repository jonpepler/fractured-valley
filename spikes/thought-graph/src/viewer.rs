//! Writes a recorded scenario as one self-contained HTML page (inline JS and SVG, no external
//! files): the graph with activation spreading hop by hop, the fox on the map, modulator bars.

use crate::graph::{lt, tag, Graph};
use crate::sim::Sim;
use crate::world::{Action, DogMode};
use std::fmt::Write;

const TEMPLATE: &str = include_str!("../viewer/template.html");

fn esc(s: &str) -> String { s.replace('\\', "\\\\").replace('"', "\\\"") }

fn action_text(a: &Action, sim: &Sim) -> String {
    match *a {
        Action::Stay => "stay".into(),
        Action::WalkTo { x, y } => {
            match sim.m.place_nodes.iter().find(|&&p| sim.m.g.anchor(p).map(|a| a.xy()) == Some((x, y))) {
                Some(&p) => format!("walk to {}", sim.m.g.names[p as usize]),
                None => format!("walk toward ({x}, {y})"),
            }
        }
        Action::Flee { from_x, from_y, speed } => format!("flee from ({from_x}, {from_y}), {speed} cells a tick"),
        Action::Snap { .. } => "snap at the dog".into(),
        Action::Eat { entity } => format!("eat {}", sim.w.entity(entity).map(|e| e.name.as_str()).unwrap_or("?")),
        Action::Sleep => "sleep".into(),
    }
}

/// A key moment to export as a picture: (tick, hop or None for the end of the tick, caption).
pub type Key = (u32, Option<usize>, String);

/// The page for a recorded scenario. `links_from` is the graph whose links are drawn: the mind's
/// own graph, or a decompiled one for the compiled form.
pub fn page(sim: &Sim, title: &str, subtitle: &str, form: &str, links_from: Option<&Graph>, keys: &[Key]) -> String {
    let g = links_from.unwrap_or(&sim.m.g);
    let mut j = String::with_capacity(1 << 20);
    write!(j, "{{\"title\":\"{}\",\"subtitle\":\"{}\",\"form\":\"{}\",\"grid\":[{},{}],", esc(title), esc(subtitle), form, crate::world::GRID_W, crate::world::GRID_H).unwrap();
    // The places drawn on the map are the fox's own (where its mind believes they are), and home
    // is the place it believes is home at the end of the recording.
    j.push_str("\"places\":[");
    let home = sim.m.home();
    let mut first_p = true;
    for (pi, &pn) in sim.m.place_nodes.iter().enumerate() {
        if !sim.m.place_learnt(pi) { continue; }
        if !first_p { j.push(',') } first_p = false;
        let (x, y) = sim.m.g.anchor(pn).unwrap().xy();
        write!(j, "{{\"name\":\"{}\",\"x\":{},\"y\":{},\"home\":{}}}", esc(&sim.m.g.names[pn as usize]), x, y, pn == home).unwrap();
    }
    j.push_str("],\"nodes\":[");
    let m = &sim.m;
    for n in 0..m.g.len() as u32 {
        if n > 0 { j.push(',') }
        let t = m.g.tags[n as usize];
        let unlearnt = m.place_nodes.iter().position(|&p| p == n).is_some_and(|pi| !m.place_learnt(pi));
        let kind = if unlearnt { "concept" } else if t & tag::PLACE != 0 { "place" } else if t & tag::ANCHORED != 0 { "thing" } else if t & tag::BODY != 0 { "body" } else { "concept" };
        let (x, y) = sim.slow_snapshots.first().and_then(|_| m.g.anchor(n)).map(|a| a.xy()).unwrap_or((-1, -1));
        let home_place = m.place_nodes.iter().position(|&p| p == n).map(|i| i as i32).unwrap_or(-1);
        // Things are drawn next to the place they are at.
        let at = (m.g.links(n)).find(|&l| m.g.ltype[l] == lt::AT).map(|l| m.g.targets[l] as i32).unwrap_or(-1);
        write!(j, "{{\"name\":\"{}\",\"kind\":\"{}\",\"x\":{},\"y\":{},\"at\":{},\"place\":{}}}", esc(&m.g.names[n as usize]), kind, x, y, at, home_place).unwrap();
    }
    j.push_str("],\"links\":[");
    let mut first = true;
    for u in 0..g.len() as u32 {
        for l in g.links(u) {
            if !first { j.push(',') } first = false;
            write!(j, "[{},{},{},{}]", u, g.targets[l], g.ltype[l], g.slow[l]).unwrap();
        }
    }
    j.push_str("],\"linkTypes\":[");
    for (i, n) in lt::NAMES.iter().enumerate() { if i > 0 { j.push(',') } write!(j, "\"{n}\"").unwrap(); }
    // Slow weights and valences at the start and after each night (graph form only).
    j.push_str("],\"snapshots\":[");
    match links_from {
        None => for (i, (t, w, v)) in sim.slow_snapshots.iter().enumerate() {
            if i > 0 { j.push(',') }
            write!(j, "{{\"t\":{t},\"w\":{:?},\"val\":{:?}}}", w, v).unwrap();
        },
        Some(g) => write!(j, "{{\"t\":0,\"w\":{:?},\"val\":{:?}}}", g.slow, g.val).unwrap(),
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
        let hops: Vec<String> = f.hops.iter().map(|h| {
            let fl: Vec<String> = h.flows.iter().map(|&(u, v, _, a)| format!("[{u},{v},{a}]")).collect();
            format!("[{}]", fl.join(","))
        }).collect();
        // Fast weights index the graph form's links; not drawn over a decompiled graph.
        let fast: Vec<String> = if links_from.is_some() { vec![] } else { f.fast.iter().map(|&(l, w)| format!("[{l},{w}]")).collect() };
        let per: Vec<String> = f.percepts.iter().map(|&(n, e)| format!("[{n},{e}]")).collect();
        let ev: Vec<String> = f.events.iter().map(|e| format!("\"{}\"", esc(e))).collect();
        write!(j, "{{\"t\":{},\"fox\":[{},{}],\"dog\":{},\"fires\":[{}],\"m\":[{},{},{},{},{},{}],\"act\":{:?},\"hops\":[{}],\"ws\":{:?},\"action\":\"{}\",\"why\":\"{}\",\"ev\":[{}],\"fast\":[{}],\"per\":[{}],\"slept\":{},\"burrows\":[{}],\"backlog\":{}}}",
            f.tick, f.fox.0, f.fox.1, dog, fires.join(","), f.mods.hunger, f.mods.arousal, f.mods.mood, f.mods.fatigue, f.pain, f.mods.stress,
            f.act, hops.join(","), f.ws, esc(&action_text(&f.action, sim)), esc(&f.why), ev.join(","), fast.join(","), per.join(","), f.slept, burrows.join(","), f.backlog).unwrap();
    }
    j.push_str("],\"keys\":[");
    for (i, (t, h, c)) in keys.iter().enumerate() {
        if i > 0 { j.push(',') }
        write!(j, "{{\"t\":{t},\"h\":{},\"label\":\"{}\"}}", h.map(|h| h as i64).unwrap_or(-1), esc(c)).unwrap();
    }
    write!(j, "],\"persist\":{},\"hopsPerTick\":{}}}", m.params.persist, m.params.hops).unwrap();
    TEMPLATE.replace("/*TITLE*/", title).replace("/*DATA*/", &j)
}
