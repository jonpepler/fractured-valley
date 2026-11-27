//! A gas burner like McCaffrey's (1979): wood gas let into a 0.3 m square at
//! ground level on dry stone, in still air. Centreline temperature against
//! McCaffrey, flame height against Heskestad, and where the heat goes.
//!     cargo run --release -p fv-core --example burner -- [kW_net] [ticks]
use fv_core::rad::Sky;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::substance::Gas;
use fv_core::transport::FaceVelocity;
use fv_core::units::*;

/// Net heat of combustion of our wood gas: its gross 16 MJ/kg less the
/// latent heat of the water it makes (CH₁.₆O₀.₇ + 1.05 O₂ → CO₂ + 0.8 H₂O:
/// 0.58 kg of water per kg, 1.31 MJ), as burner heat release is booked.
const NET_MJ_PER_KG: f64 = 16.0 - 0.58 * 2.257;

fn mccaffrey(z: f64, q: f64, t0: f64) -> f64 {
    let s = z / q.powf(0.4);
    let (k, eta) = if s < 0.08 { (6.8, 0.5) } else if s < 0.2 { (1.9, 0.0) } else { (1.1, -1.0 / 3.0) };
    (k / 0.9f64).powi(2) * s.powf(2.0 * eta - 1.0) * t0 / (2.0 * 9.81)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let kw: f64 = args.first().and_then(|v| v.parse().ok()).unwrap_or(50.0);
    let ticks: u64 = args.get(1).and_then(|v| v.parse().ok()).unwrap_or(700);
    let (w, cells) = fv_core::scene::burner(32, 32);
    let base = cells[0].2;
    let mut sim = Sim::new(w, Sky::NIGHT);
    sim.flow.wind_mean = [0, 0];
    sim.flames = Some(Vec::new());
    // fuel per cell per tick, µg
    let per = (kw * 1e3 / (NET_MJ_PER_KG * 1e6) * 0.05 * 1e9 / cells.len() as f64).round() as i64;
    let mut seq = 0;
    for t in 0..ticks { for &(x, y, z) in &cells {
        seq += 1;
        sim.schedule(Scheduled { tick: t, peer: 0, seq, command: Command::Gas { x: x as u16, y: y as u16, z: z as u16, mass: Mass(per) } });
        if t < 40 { seq += 1; sim.schedule(Scheduled { tick: t, peer: 0, seq, command: Command::Heat { x: x as u16, y: y as u16, z: z as u16, energy: joules(200) } }) }
    }}
    let (cx, cy) = (cells[4].0, cells[4].1);
    let nz = sim.world.nz;
    let warm = 200u64;
    let (mut temps, mut temps9, mut tops, mut heat_z, mut up) = (vec![0.0f64; nz], vec![0.0f64; nz], Vec::new(), vec![0.0f64; nz], vec![0.0f64; nz]);
    let mut released = 0.0f64;
    let last = 0i128;
    let mut plume1m = 0.0f64;
    for tick in 1..=ticks {
        sim.step();
        assert_eq!(sim.energy_gap(), 0);
        assert_eq!(sim.world.first_impossible(), None);
        // chemical released: chemical that went away, counting what came in by command
        let w = &sim.world;
        let fuel_in = sim.books.commands_mass as f64 * 16.0; // µg × MJ/kg = µJ
        let chem = sim.totals().chemical as f64 - sim.books.edge_chemical() as f64 - fuel_in;
        let _ = (chem, last);
        if tick > warm { released += sim.flames.as_ref().unwrap().iter().sum::<i64>() as f64 / 0.35 }
        if std::env::var("DBG").is_ok() && (tick % 50 == 0 || (tick > 195 && tick < 240)) {
            let fl = sim.flames.as_ref().unwrap();
            let fuel_box: i64 = w.cells.iter().map(|c| c.gas[Gas::Fuel as usize].0).sum();
            let c = w.get(cx, cy, base);
            let hot = w.cells.iter().filter(|c| c.gas_volume_ml(&w.subs) > 0 && c.gas_temperature() >= fv_core::chem::IGNITION).count();
            let lit = fv_core::chem::flame_reach(w).iter().filter(|&&l| l).count();
            print!("hot {hot} lit {lit} ");
            println!("tick {tick}: in {:.2} g, in box {:.2} g, edge fuel {:.2} g, flames {:.1} kW, centre cell {:.0} °C fuel {} O2 {}", sim.books.commands_mass as f64 * 1e-6, fuel_box as f64 * 1e-6, sim.books.edge_gas[Gas::Fuel as usize] as f64 * 1e-6, fl.iter().sum::<i64>() as f64 / 0.35 * 1e-6 * 20.0 / 1e3, c.gas_temperature().celsius_f64(), c.gas[Gas::Fuel as usize].0, c.gas[Gas::Oxygen as usize].0);
        }
        if tick <= warm { continue }
        let fl = sim.flames.as_ref().unwrap();
        let mut top = base;
        for z in base..nz {
            let mut t9 = 0.0;
            for dy in 0..3 { for dx in 0..3 {
                let (x, y) = (cx + dx - 1, cy + dy - 1);
                let i = w.idx(x, y, z);
                t9 += w.cells[i].gas_temperature().celsius_f64() / 9.0;
            }}
            for y in cy - 3..cy + 4 { for x in cx - 3..cx + 4 {
                let i = w.idx(x, y, z);
                heat_z[z] += fl[i] as f64 / 0.35 * 1e-6;
                if fl[i] as f64 / 0.35 > 10e6 || w.cells[i].gas_temperature().celsius_f64() > 600.0 { top = top.max(z + 1) }
            }}
            temps[z] += w.get(cx, cy, z).gas_temperature().celsius_f64();
            temps9[z] += t9;
            up[z] += sim.flow.face(cx as i64, cy as i64, z as i64, 2) as f64 / 1000.0;
        }
        tops.push((top - base) as f64 * 0.1);
        // sensible heat up through 1 m (top of layer base+9)
        let z = base + 9;
        for y in 0..w.ny { for x in 0..w.nx {
            let u = sim.flow.face(x as i64, y as i64, z as i64, 2) as f64 / 1000.0;
            let d = if u > 0.0 { w.get(x, y, z) } else { w.get(x, y, z + 1) };
            let ml = d.gas_volume_ml(&w.subs) as f64;
            if ml <= 0.0 { continue }
            let excess = (d.energy.0 - d.latent().0 - (d.gas_capacity() * w.ambient).0) as f64 * 1e-6;
            plume1m += excess * u * 0.01 / (ml * 1e-6) / 1e3;
        }}
    }
    let n = (ticks - warm) as f64;
    // Heat the flames released (their light is 35 % of it), gross.
    let q_gross = released * 1e-6 / (n * 0.05) / 1e3;
    let q_net = q_gross * NET_MJ_PER_KG / 16.0;
    tops.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let flame = tops[tops.len() / 2];
    let d = 0.3;
    let hesk = 0.235 * q_net.powf(0.4) - 1.02 * d;
    let total: f64 = heat_z.iter().sum();
    println!("burner {kw:.0} kW asked: released {q_gross:.1} kW gross, {q_net:.1} kW net; plume sensible heat through 1 m {:.1} kW ({:.0} % of net)", plume1m / n, 100.0 * plume1m / n / q_net);
    println!("flame height {flame:.2} m, Heskestad {hesk:.2} m; flame heat by height: {}", heat_z.iter().enumerate().filter(|(_, v)| **v / total.max(1e-9) > 0.01).map(|(z, v)| format!("{:.1}m:{:.2}", (z - base) as f64 * 0.1, v / total)).collect::<Vec<_>>().join(" "));
    println!("height m | centre °C rise | 3x3 mean rise | McCaffrey rise | up m/s");
    let t0 = 15.0;
    for z in base..(base + 16).min(nz) {
        let h = (z - base) as f64 * 0.1 + 0.05;
        println!("  {h:4.2} | {:6.0} | {:6.0} | {:6.0} | {:5.2}", temps[z] / n - t0, temps9[z] / n - t0, mccaffrey(h, q_net, t0 + 273.15), up[z] / n);
    }
}
