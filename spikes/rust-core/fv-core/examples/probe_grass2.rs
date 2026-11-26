use fv_core::cell::Cell;
use fv_core::rad::Sky;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::substance::*;
use fv_core::units::*;
use fv_core::world::World;
fn e(w: &World) -> (i64, i64, i64) { let c = w.get(16, 3, 4); (c.skin.0 / 1000, c.solid_energy.0 / 1000, c.energy.0 / 1000) }
fn main() {
    let n = 32;
    let mut w = World::new(n, n, 16, 9, celsius(15));
    let subs = w.subs.clone();
    let soil = Cell::solid_of(SOIL, 1000, 150, celsius(15), &subs);
    let grass = Cell::solid_of(GRASS, 1000, 100, celsius(15), &subs);
    for z in 0..4 { for y in 0..n { for x in 0..n { w.set(x, y, z, soil) } } }
    for y in 2..30 { for x in 2..30 { w.set(x, y, 4, grass) } }
    let mut sim = Sim::new(w, Sky::NIGHT);
    for t in 0..100 { for x in 2..30u16 { sim.schedule(Scheduled { tick: t, peer: 0, seq: x as u32, command: Command::Heat { x, y: 2, z: 4, energy: joules(400) } }) } }
    let mut sums = [[0i64; 3]; 4];
    for tick in 0..200 {
        let a = e(&sim.world);
        sim.step_before_diffuse(); let b = e(&sim.world);
        fv_core::transport::diffuse(&mut sim.world, &sim.flow); let c = e(&sim.world);
        fv_core::chem::react(&mut sim.world); let d = e(&sim.world);
        fv_core::rad::radiate(&mut sim.world, &sim.sky, &mut sim.light); let f = e(&sim.world);
        sim.world.tick += 1;
        for (k, (p, q)) in [(a, b), (b, c), (c, d), (d, f)].iter().enumerate() { sums[k][0] += q.0 - p.0; sums[k][1] += q.1 - p.1; sums[k][2] += q.2 - p.2 }
        if tick % 40 == 39 {
            println!("{:.0}s neighbour J [skin, core, gas] by phase: advect {:?} diffuse {:?} react {:?} radiate {:?} | burning row skin {:.0}C", (tick + 1) as f64 / 20.0, sums[0].map(|v| v / 1000), sums[1].map(|v| v / 1000), sums[2].map(|v| v / 1000), sums[3].map(|v| v / 1000), sim.world.get(16, 2, 4).surface_temperature(&sim.world.subs).celsius_f64());
            sums = [[0; 3]; 4];
        }
    }
}
