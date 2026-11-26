use fv_core::cell::Cell;
use fv_core::ledger::Totals;
use fv_core::rad::{radiate, Sky};
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::substance::*;
use fv_core::transport::{advect, diffuse};
use fv_core::units::*;
use fv_core::world::World;
fn main() {
    let n = 32;
    let mut w = World::new(n, n, 32, 7, celsius(20));
    let soil = Cell::solid_of(SOIL, 1000, 150, celsius(20), &w.subs);
    let grass = Cell::solid_of(GRASS, 1000, 150, celsius(20), &w.subs);
    let post = Cell::solid_of(WOOD, 1000, 120, celsius(20), &w.subs);
    for y in 0..n { for x in 0..n { for z in 0..6 { w.set(x, y, z, soil) } w.set(x, y, 6, grass); } }
    for z in 6..16 { for y in 15..17 { for x in 15..17 { w.set(x, y, z, post) } } }
    let mut sim = Sim::new(w, Sky::NIGHT);
    for t in 0..60 { sim.schedule(Scheduled { tick: t, peer: 0, seq: 0, command: Command::Heat { x: 10, y: 16, z: 6, energy: joules(250) } }) }
    for _ in 0..98 { sim.step() }
    println!("gap before: {}", sim.energy_gap());
    // One tick by hand, checking each phase.
    let check = |label: &str, w: &World, before: &Totals| {
        let t = Totals::of(w);
        let mut edge = 0i128; let mut rad = 0i128;
        for tl in &w.tallies { edge += tl.energy_in as i128; rad += tl.radiation_in as i128 - tl.radiation_out as i128 }
        println!("{label}: change={} edge={} rad={} unexplained={}", t.energy() - before.energy(), edge, rad, t.energy() - before.energy() - edge - rad);
    };
    let s = &mut sim;
    let w = &mut s.world;
    for t in w.tallies.iter_mut() { *t = Default::default() }
    s.flow.step(w);
    let b = Totals::of(w); advect(w, &s.flow, s.advect_steps); check("advect", w, &b);
    for t in w.tallies.iter_mut() { *t = Default::default() }
    let b = Totals::of(w); diffuse(w, &s.flow); check("diffuse", w, &b);
    for t in w.tallies.iter_mut() { *t = Default::default() }
    let b = Totals::of(w); fv_core::chem::react(w); check("react", w, &b);
    for t in w.tallies.iter_mut() { *t = Default::default() }
    let b = Totals::of(w); radiate(w, &s.sky, &mut s.light); check("radiate", w, &b);
}
