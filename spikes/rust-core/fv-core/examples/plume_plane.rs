//! Exact heat carried up across horizontal planes over the still-air
//! campfire, by each phase: the energy above the plane before and after
//! diffusion and advection, less the heat of the gas mass that crossed at
//! ambient temperature (sensible heat above ambient, plus latent).
use fv_core::rad::Sky;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::substance::*;
use fv_core::units::joules;
fn main() {
    let (n, nz, torch_ticks, ticks) = (32usize, 32usize, 200u64, 700u64);
    let s = fv_core::scene::campfire(n, nz);
    let mut sim = Sim::new(s.world, Sky::NIGHT);
    sim.flow.wind_mean = [0, 0];
    let mut seq = 0;
    for tick in 0..torch_ticks { for &(x, y, z) in &s.torch {
        sim.schedule(Scheduled { tick, peer: 0, seq, command: Command::Heat { x: x as u16, y: y as u16, z: z as u16, energy: joules(500) } });
        seq += 1;
    }}
    let planes = [6usize, 8, 11, 14, 18, 22];
    // energy (gas, J, absolute incl. latent), latent (J), gas mass (kg) above each plane
    let above = |sim: &Sim, z0: usize| -> (f64, f64, f64) {
        let w = &sim.world; let (mut e, mut l, mut m) = (0.0, 0.0, 0.0);
        for z in z0..w.nz { for y in 0..w.ny { for x in 0..w.nx { let c = w.get(x, y, z); e += c.energy.0 as f64 * 1e-6; l += c.latent().0 as f64 * 1e-6; m += c.gas_mass().0 as f64 * 1e-9 } } }
        (e, l, m)
    };
    let mut acc = vec![[0.0f64; 3]; planes.len()]; // diffuse sensible, advect sensible, latent both
    let cp_amb = 1005.0 * sim.world.ambient.0 as f64 / 1000.0; // J/kg of air at ambient
    for tick in 1..=ticks {
        if tick <= torch_ticks { sim.step(); continue }
        sim.step_before_diffuse();
        let b0: Vec<_> = planes.iter().map(|&z| above(&sim, z)).collect();
        fv_core::transport::diffuse(&mut sim.world, &sim.flow);
        let b1: Vec<_> = planes.iter().map(|&z| above(&sim, z)).collect();
        sim.flow.step(&sim.world);
        fv_core::transport::advect(&mut sim.world, &sim.flow, sim.advect_steps);
        let b2: Vec<_> = planes.iter().map(|&z| above(&sim, z)).collect();
        for k in 0..planes.len() {
            let sens = |a: (f64, f64, f64), b: (f64, f64, f64)| (b.0 - b.1) - (a.0 - a.1) - (b.2 - a.2) * cp_amb;
            acc[k][0] += sens(b0[k], b1[k]);
            acc[k][1] += sens(b1[k], b2[k]);
            acc[k][2] += (b2[k].1 - b0[k].1);
        }
        fv_core::chem::react(&mut sim.world);
        fv_core::rad::radiate_by(sim.radiation, &mut sim.world, &sim.sky, &mut sim.light);
        sim.world.tick += 1;
    }
    let secs = (ticks - torch_ticks) as f64 * 0.05;
    for (k, &z) in planes.iter().enumerate() {
        println!("plane {:.1} m above the bed's base: sensible heat up by mixing/conduction {:.1} kW, by the flow {:.1} kW; latent {:.1} kW", (z - 4) as f64 * 0.1, acc[k][0] / secs / 1e3, acc[k][1] / secs / 1e3, acc[k][2] / secs / 1e3);
    }
}
