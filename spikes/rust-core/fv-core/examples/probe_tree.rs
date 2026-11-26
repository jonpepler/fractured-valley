use fv_core::rad::Sky;
use fv_core::scene::crib_and_tree;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::substance::*;
use fv_core::units::*;
fn main() {
    let dead = std::env::args().any(|a| a == "dead");
    let (w, torch) = crib_and_tree(4, 48, 48, dead);
    let mut sim = Sim::new(w, Sky::afternoon());
    for t in 0..200 { for (i, p) in torch.iter().enumerate().filter(|(i, _)| i % 3 == 0) { sim.schedule(Scheduled { tick: t, peer: 0, seq: i as u32, command: Command::Heat { x: p[0] as u16, y: p[1] as u16, z: p[2] as u16, energy: joules(500) } }) } }
    let t0 = std::time::Instant::now();
    let ticks: u64 = std::env::var("TICKS").ok().and_then(|v| v.parse().ok()).unwrap_or(1600);
    for tick in 0..=ticks {
        let before = sim.world.cells.clone();
        sim.step();
        if std::env::var("CATCH").is_ok() {
            let w = &sim.world;
            if let Some(i) = (0..w.cells.len()).find(|&i| w.cells[i].gas_mass().0 > 20_000_000 || w.cells[i].gas.iter().any(|m| m.0 < 0) || w.cells[i].skin.0 < 0 || w.cells[i].energy.0 < 0 || w.cells[i].solid_energy.0 < 0) {
                let (c, b) = (&w.cells[i], &before[i]);
                println!("tick {tick}: pieces {} landed? cell {i} sub {} gasT {:.0}C E {} -> {} gas mass {} -> {} skin {} -> {} | before gasT {:.0}C", sim.bodies.pieces.len(), c.substance, c.gas_temperature().celsius_f64(), b.energy.0, c.energy.0, b.gas_mass().0, c.gas_mass().0, b.skin.0, c.skin.0, b.gas_temperature().celsius_f64());
                println!("  gas now {:?}\n  gas before {:?}", c.gas.map(|m| m.0), b.gas.map(|m| m.0));
                break;
            }
        }
        if tick % 200 == 0 {
            let w = &sim.world;
            let (mut leaves_burnt, mut leaves, mut hot_top, mut wood_vol, mut maxt) = (0, 0, 0usize, 0i64, 0i64);
            for z in 0..w.nz { for y in 0..w.ny { for x in 0..w.nx {
                let c = w.get(x, y, z);
                if c.substance == LEAVES { leaves += 1; if c.solid[0].0 < 1_500_000 { leaves_burnt += 1 } }
                if c.substance == WOOD { wood_vol += c.solid[0].0 }
                let tg = c.gas_temperature().0;
                maxt = maxt.max(tg);
                if tg > 900_000 { hot_top = hot_top.max(z) }
            }}}
            println!("{tick:5} gap {} | maxgas {:.0}C flame top z{} | leaves burnt {}/{} | wood volatile {:.1} kg | {:.0} ms/tick | in flight {}",
                sim.energy_gap(), maxt as f64 / 1000.0 - 273.15, hot_top, leaves_burnt, leaves, wood_vol as f64 / 1e9, t0.elapsed().as_millis() as f64 / (tick + 1) as f64, sim.bodies.pieces.len());
        }
    }
}
