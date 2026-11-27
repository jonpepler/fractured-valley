//! The valley scene (seed 5, 32³, lit 40 ticks): the first tick and phase a
//! store goes negative, and which store.
use fv_core::rad::Sky;
use fv_core::scene::valley;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::units::*;
fn neg(w: &fv_core::world::World) -> Option<(usize, String)> {
    for (i, c) in w.cells.iter().enumerate() {
        let mut bad = vec![];
        for (k, m) in c.gas.iter().enumerate() { if m.0 < 0 { bad.push(format!("gas{k}={}", m.0)) } }
        for (k, m) in c.solid.iter().enumerate() { if m.0 < 0 { bad.push(format!("solid{k}={}", m.0)) } }
        if c.water.0 < 0 { bad.push(format!("water={}", c.water.0)) }
        if c.energy.0 < 0 { bad.push(format!("energy={}", c.energy.0)) }
        if c.gas_capacity().0 > 0 && c.gas_temperature().0 < 260_000 { bad.push(format!("gas at {:.0} °C", c.gas_temperature().celsius_f64())) }
        if c.solid_energy.0 < 0 { bad.push(format!("solid_energy={}", c.solid_energy.0)) }
        if c.skin.0 < 0 { bad.push(format!("skin={}", c.skin.0)) }
        if !bad.is_empty() { return Some((i, bad.join(" "))) }
    }
    None
}
fn main() {
    let v = valley(5, 32, 32, 60);
    let (cx, cy) = (v.crib.0.min(28), v.crib.1.min(28));
    let gz = v.ground_at(cx, cy) as u16 + 1;
    let mut sim = Sim::new(v.world, Sky::afternoon());
    for t in 0..40 { sim.schedule(Scheduled { tick: t, peer: 0, seq: 0, command: Command::Heat { x: cx as u16, y: cy as u16, z: gz, energy: joules(250) } }) }
    for tick in 1..=300u64 {
        let before = sim.world.cells.clone();
        if tick == 45 {
            let w = &sim.world; let i = 11461usize;
            let sides = fv_core::transport::sides(w);
            let (b, l) = (i / 512, i % 512); let (ox, oy, oz) = w.brick_origin(b); let (lx, ly, lz) = fv_core::world::World::local_xyz(l);
            let (x, y, z) = (ox + lx, oy + ly, oz + lz);
            let c = w.get(x, y, z);
            println!("cell ({x},{y},{z}) gas E {} latent {} cap*amb {} T {:.1} C, skin T {:.0} C, fuel_g {}, flame_t {}", c.energy.0, c.latent().0, (c.gas_capacity() * w.ambient).0, c.gas_temperature().celsius_f64(), c.surface_temperature(&w.subs).celsius_f64(), sides[i].fuel_g, sides[i].flame_t.0);
            for axis in 0..3 { for dir in [-1i64, 1] {
                let mut p = [x as i64, y as i64, z as i64]; p[axis] += dir;
                if !w.inside(p[0], p[1], p[2]) { continue }
                let j = w.idx(p[0] as usize, p[1] as usize, p[2] as usize); let n = &w.cells[j];
                use fv_core::transport::FaceVelocity;
                let f = if dir > 0 { fv_core::transport::diffuse_face(c, &sides[i], n, &sides[j], 0, axis) } else { fv_core::transport::diffuse_face(n, &sides[j], c, &sides[i], 0, axis) };
                let s = -dir;
                println!("  face axis {axis} dir {dir}: nbr sub {} T {:.0} C | into me: cond {} carried {} flame {} gas {:?}", n.substance, n.gas_temperature().celsius_f64(), f.energy.0 * s, f.carried.0 * s, f.flame.0 * s, f.gas.map(|m| m.0 * s));
            }}
        }
        sim.step_before_diffuse();
        let mut phase = "";
        let mut found = None;
        fv_core::transport::diffuse(&mut sim.world, &sim.flow);
        if found.is_none() { found = neg(&sim.world); phase = "diffuse" }
        sim.flow.step(&sim.world);
        fv_core::transport::advect(&mut sim.world, &sim.flow, sim.advect_steps);
        if found.is_none() { found = neg(&sim.world); phase = "advect" }
        fv_core::chem::react_in(&mut sim.world, &sim.flow);
        if found.is_none() { found = neg(&sim.world); phase = "react" }
        fv_core::rad::radiate_by(sim.radiation, &mut sim.world, &sim.sky, &mut sim.light);
        if found.is_none() { found = neg(&sim.world); phase = "radiate" }
        if let Some((i, what)) = found {
            let c = &before[i];
            println!("tick {tick}, after {phase}: cell {i} ({:?}) {what}; before: sub {} gas {:?} T {:.0} °C", sim.world.cells.len(), c.substance, c.gas.map(|m| m.0), c.gas_temperature().celsius_f64());
            return;
        }
        sim.world.tick += 1;
    }
    println!("nothing negative in 300 ticks (phases run by hand: structure and bodies skipped)");
}
