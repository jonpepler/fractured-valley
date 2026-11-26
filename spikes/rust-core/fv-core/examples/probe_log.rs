use fv_core::rad::Sky;
use fv_core::scene::crib_and_tree;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::units::*;
fn show(tag: &str, sim: &Sim, i: usize) {
    let c = &sim.world.cells[i]; let s = &sim.world.subs;
    println!("{tag:8} flame {:>9} gasmass {:>8} skin {:>14} ({:.0}C) core {:>14} ({:.0}C) gasE {:>14} water {:>9} vap {:>9} drops {:>9}", c.flame.0, c.gas_mass().0, c.skin.0, c.surface_temperature(s).celsius_f64(), c.solid_energy.0, c.solid_temperature(s).celsius_f64(), c.energy.0, c.water.0, c.gas[3].0, c.gas[6].0);
}
fn main() {
    let (w, torch) = crib_and_tree(4, 48, 48, true);
    let mut sim = Sim::new(w, Sky::afternoon());
    for t in 0..200 { for (i, p) in torch.iter().enumerate().filter(|(i, _)| i % 3 == 0) { sim.schedule(Scheduled { tick: t, peer: 0, seq: i as u32, command: Command::Heat { x: p[0] as u16, y: p[1] as u16, z: p[2] as u16, energy: joules(500) } }) } }
    let i = 29079;
    for _ in 0..2246 { sim.step() }
    show("start", &sim, i);
    sim.step_before_diffuse(); show("cmds", &sim, i);
    fv_core::transport::diffuse(&mut sim.world, &sim.flow); show("diffuse", &sim, i);
    sim.flow.step(&mut sim.world);
    fv_core::transport::advect(&mut sim.world, &sim.flow, sim.advect_steps); show("advect", &sim, i);
    fv_core::chem::react(&mut sim.world); show("react", &sim, i);
    fv_core::rad::radiate_by(sim.radiation, &mut sim.world, &sim.sky, &mut sim.light); show("radiate", &sim, i);
}
