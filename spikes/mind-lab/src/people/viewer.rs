//! A recorded teaching scenario as one self-contained page: one person's mind (drawn from its
//! expanded form), the valley with the other people and the things it holds, modulator bars.
//! The page is the fox viewer's, with people and things labelled on the map
//! (`viewer/people-template.html`).

use super::sim::Valley;
use super::world::{name, GRID_H, GRID_W};
use crate::fox::viewer::{expand_any, Key};
use crate::graph::{link_pos, link_src, lt, tag, LinkKey};
use crate::mind::Action;
use crate::store::{Expanded, Store};
use crate::think::Form;
use std::fmt::Write;

const TEMPLATE: &str = include_str!("../../viewer/people-template.html");
const DANGER_TEMPLATE: &str = include_str!("../../viewer/danger-template.html");

fn esc(s: &str) -> String { s.replace('\\', "\\\\").replace('"', "\\\"") }

fn action_text(a: &Action, v: &Valley, who: usize) -> String {
    let m = &v.folk[who].m;
    match *a {
        Action::Stay => "stay".into(),
        Action::WalkTo { x, y } => match m.place_nodes.iter().find(|&&p| m.store.anchor(p).map(|a| a.xy()) == Some((x, y))) {
            Some(&p) => format!("walk to {}", m.name(p)),
            None => format!("walk toward ({x}, {y})"),
        },
        Action::Flee { .. } => "flee".into(),
        Action::Snap { .. } => "snap".into(),
        Action::Eat { entity } => { let t = v.w.things.iter().find(|t| t.id == entity); format!("{} {}", if t.is_some_and(|t| t.kind == super::world::Kind::Berry) { "eat from" } else { "drink from" }, t.map(|t| t.name.as_str()).unwrap_or("?")) }
        Action::Sleep => "sleep".into(),
        Action::Hold { .. } => "hold its ground".into(),
        Action::Freeze => "freeze".into(),
        Action::Call => "call for help".into(),
        Action::Charge { .. } => "close in to fight".into(),
    }
}

/// The page for a recorded valley (recording `who`'s mind).
pub fn page(v: &Valley, title: &str, subtitle: &str, keys: &[Key]) -> String { page_with(v, title, subtitle, keys, false) }
/// Stage 3: the danger page: the boar on the map, pain in place of thirst, and the focus of a
/// held intent as a bar (`viewer/danger-template.html`).
pub fn danger_page(v: &Valley, title: &str, subtitle: &str, keys: &[Key]) -> String { page_with(v, title, subtitle, keys, true) }
fn page_with(v: &Valley, title: &str, subtitle: &str, keys: &[Key], danger: bool) -> String {
    let who = v.record.expect("recorded");
    let p = &v.folk[who];
    let m = &p.m;
    let ex: Expanded = expand_any(&m.store);
    let li = |k: LinkKey| ex.offsets[link_src(k) as usize] as usize + link_pos(k);
    let mut j = String::with_capacity(1 << 20);
    write!(j, "{{\"title\":\"{}\",\"subtitle\":\"{}\",\"form\":\"runtime (shared base + overlay), drawn expanded\",\"me\":\"{}\",\"drive\":\"{}\",\"grid\":[{},{}],", esc(title), esc(subtitle), &name(who as usize)[..1], if danger { "pain" } else { "thirst" }, GRID_W, GRID_H).unwrap();
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
        let kind = if t & tag::PLACE != 0 { "place" } else if t & tag::ANCHORED != 0 { "thing" } else if t & tag::BODY != 0 { "body" } else { "concept" };
        let (x, y) = ex.anchor(n).map(|a| a.xy()).unwrap_or((-1, -1));
        let pl = m.place_nodes.iter().position(|&q| q == n).map(|i| i as i32).unwrap_or(-1);
        let at = ex.range(n).find(|&l| ex.ltype[l] == lt::AT).map(|l| ex.targets[l] as i32).unwrap_or(-1);
        write!(j, "{{\"name\":\"{}\",\"kind\":\"{}\",\"x\":{},\"y\":{},\"at\":{},\"place\":{}}}", esc(&ex.names[n as usize]), kind, x, y, at, pl).unwrap();
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
    for (i, (t, w, val)) in v.snapshots.iter().enumerate() {
        if i > 0 { j.push(',') }
        let mut w = w.clone(); w.resize(ex.slow.len(), 0);
        let mut val = val.clone(); val.resize(ex.val.len(), 0);
        write!(j, "{{\"t\":{t},\"w\":{:?},\"val\":{:?}}}", w, val).unwrap();
    }
    j.push_str("],\"frames\":[");
    for (i, f) in v.frames.iter().enumerate() {
        if i > 0 { j.push(',') }
        let marks: Vec<String> = f.others.iter().chain(f.things.iter()).map(|(x, y, l)| format!("[{x},{y},\"{}\"]", esc(l))).collect();
        let hops: Vec<String> = f.hops.iter().map(|h| { let fl: Vec<String> = h.flows.iter().filter(|x| (x.0 as usize) < ex.len() && (x.1 as usize) < ex.len()).map(|&(u, w, _, a)| format!("[{u},{w},{a}]")).collect(); format!("[{}]", fl.join(",")) }).collect();
        let fast: Vec<String> = f.fast.iter().filter(|&&(k, _)| (link_src(k) as usize) < ex.len() && li(k) < ex.offsets[link_src(k) as usize + 1] as usize).map(|&(k, w)| format!("[{},{w}]", li(k))).collect();
        let per: Vec<String> = f.percepts.iter().filter(|x| (x.0 as usize) < ex.len()).map(|&(n, e)| format!("[{n},{e}]")).collect();
        let ev: Vec<String> = f.events.iter().map(|e| format!("\"{}\"", esc(e))).collect();
        let mut act = vec![0i32; ex.len()];
        for &(n, a) in &f.act { if (n as usize) < act.len() { act[n as usize] = a; } }
        let ws: Vec<u32> = f.ws.iter().copied().filter(|&n| (n as usize) < ex.len()).collect();
        write!(j, "{{\"t\":{},\"fox\":[{},{}],\"dog\":{},\"fires\":[],\"m\":[{},{},{},{},{},{}],\"act\":{:?},\"hops\":[{}],\"ws\":{:?},\"action\":\"{}\",\"why\":\"{}\",\"ev\":[{}],\"fast\":[{}],\"per\":[{}],\"slept\":{},\"burrows\":[{}],\"backlog\":{}}}",
            f.tick, f.me.0, f.me.1, f.boar.filter(|_| danger).map(|(x, y)| format!("[{x},{y},\"\"]")).unwrap_or("null".into()),
            if danger { f.pain } else { f.thirst }, f.mods.arousal, f.mods.mood, f.mods.fatigue, if danger { f.focus } else { 0 }, f.mods.stress,
            act, hops.join(","), ws, esc(&action_text(&f.action, v, who)), esc(&f.why), ev.join(","), fast.join(","), per.join(","), f.slept, marks.join(","), f.backlog).unwrap();
    }
    j.push_str("],\"keys\":[");
    for (i, (t, h, c)) in keys.iter().enumerate() {
        if i > 0 { j.push(',') }
        write!(j, "{{\"t\":{t},\"h\":{},\"label\":\"{}\"}}", h.map(|h| h as i64).unwrap_or(-1), esc(c)).unwrap();
    }
    write!(j, "],\"persist\":{},\"hopsPerTick\":{}}}", m.params.persist, m.params.hops).unwrap();
    (if danger { DANGER_TEMPLATE } else { TEMPLATE }).replace("/*TITLE*/", title).replace("/*DATA*/", &j)
}
