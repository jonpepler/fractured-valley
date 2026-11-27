//! The GPU diffusion phase, put in the sim in place of the CPU's, gives the
//! same world every tick: the same hash and exact books. Skipped (with a
//! note) where there's no GPU adapter at all; on a machine without a GPU,
//! Mesa's lavapipe will do:
//! `VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json cargo test --release -p fv-gpu`
use fv_core::rad::Sky;
use fv_core::scene::{campfire, valley};
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::units::*;
use fv_gpu::GpuDiffusion;
use std::sync::{Arc, Mutex};

/// No adapter: skip, unless FV_GPU_REQUIRED is set (CI), where it fails.
fn no_gpu(e: String) {
    if std::env::var_os("FV_GPU_REQUIRED").is_some() { panic!("FV_GPU_REQUIRED, but {e}") }
    eprintln!("skipped: {e}");
}

fn compare(make: impl Fn() -> Sim, ticks: usize, burning: bool) {
    let mut cpu = make();
    let mut gpu = make();
    let g = match GpuDiffusion::new(&gpu.world) {
        Ok(g) => Arc::new(Mutex::new(g)),
        Err(e) => { no_gpu(e); return }
    };
    gpu.diffuse_hook = Some(Box::new(move |w, flow| g.lock().unwrap().diffuse(w, flow)));
    for t in 0..ticks {
        cpu.step();
        gpu.step();
        assert_eq!(cpu.hash(), gpu.hash(), "tick {t}: the GPU's world differs");
        assert_eq!(cpu.world.tallies, gpu.world.tallies, "tick {t}: the edge tallies differ");
        assert_eq!((gpu.energy_gap(), gpu.mass_gap()), (0, 0), "tick {t}: books");
        assert_eq!(gpu.world.first_impossible(), None);
    }
    // The flame paths (contact, lean, pressure) were exercised.
    if burning { assert!(gpu.world.cells.iter().any(|c| c.lit), "nothing burnt") }
}

#[test]
fn a_campfire_burns_the_same_on_the_gpu() {
    compare(|| {
        let f = campfire(24, 16);
        let mut sim = Sim::new(f.world, Sky::afternoon());
        for t in 0..30 { for (i, &(x, y, z)) in f.torch.iter().enumerate() { sim.schedule(Scheduled { tick: t, peer: 0, seq: i as u32, command: Command::Heat { x: x as u16, y: y as u16, z: z as u16, energy: joules(2_000) } }) } }
        sim
    }, 60, true);
}

#[test]
fn a_lit_valley_is_the_same_on_the_gpu() {
    compare(|| {
        let v = valley(7, 32, 32, 60);
        let (cx, cy) = (v.crib.0.min(28), v.crib.1.min(28));
        let gz = v.ground_at(cx, cy) as u16 + 1;
        let mut sim = Sim::new(v.world, Sky::afternoon());
        for t in 0..40 { sim.schedule(Scheduled { tick: t, peer: 0, seq: 0, command: Command::Heat { x: cx as u16, y: cy as u16, z: gz, energy: joules(250) } }) }
        sim
    }, 60, false);
}

/// The same on the brick pool, with the bricks in a shuffled slot order.
fn compare_pool(make: impl Fn() -> Sim, ticks: usize, mode: fv_gpu::pool::Mode, compact: bool) {
    let mut cpu = make();
    let mut gpu = make();
    let n = gpu.world.bricks();
    let g = match fv_gpu::pool::PoolDiffusion::with_mode(&gpu.world, fv_gpu::pool::shuffled(n, 99), mode, compact) {
        Ok(g) => Arc::new(Mutex::new(g)),
        Err(e) => { no_gpu(e); return }
    };
    gpu.diffuse_hook = Some(Box::new(move |w, flow| g.lock().unwrap().diffuse(w, flow)));
    for t in 0..ticks {
        cpu.step();
        gpu.step();
        assert_eq!(cpu.hash(), gpu.hash(), "tick {t}: the pool's world differs");
        assert_eq!(cpu.world.tallies, gpu.world.tallies, "tick {t}: the edge tallies differ");
        assert_eq!((gpu.energy_gap(), gpu.mass_gap()), (0, 0), "tick {t}: books");
    }
    assert!(gpu.world.cells.iter().any(|c| c.lit), "nothing burnt");
}

fn burning_campfire() -> Sim {
    let f = campfire(24, 16);
    let mut sim = Sim::new(f.world, Sky::afternoon());
    for t in 0..30 { for (i, &(x, y, z)) in f.torch.iter().enumerate() { sim.schedule(Scheduled { tick: t, peer: 0, seq: i as u32, command: Command::Heat { x: x as u16, y: y as u16, z: z as u16, energy: joules(2_000) } }) } }
    sim
}

#[test]
fn a_campfire_burns_the_same_on_a_shuffled_brick_pool() { compare_pool(burning_campfire, 60, fv_gpu::pool::Mode::Workgroup, false) }

#[test]
fn a_campfire_burns_the_same_on_a_pool_with_a_face_scratch_and_compact_cells() { compare_pool(burning_campfire, 60, fv_gpu::pool::Mode::Scratch, true) }

/// Diffusion and carrying both on the GPU, the state staying there between
/// the two: every tick the same as the CPU's.
fn compare_resident(make: impl Fn() -> Sim, ticks: usize, burning: bool) {
    let mut cpu = make();
    let mut gpu = make();
    let n = gpu.world.bricks();
    let g = match fv_gpu::pool::PoolDiffusion::with_mode(&gpu.world, fv_gpu::pool::shuffled(n, 5), fv_gpu::pool::Mode::Scratch, true) {
        Ok(g) => Arc::new(Mutex::new(g)),
        Err(e) => { no_gpu(e); return }
    };
    let g2 = g.clone();
    gpu.diffuse_hook = Some(Box::new(move |w, flow| g.lock().unwrap().diffuse(w, flow)));
    gpu.advect_hook = Some(Box::new(move |w, flow, steps| g2.lock().unwrap().advect(w, flow, steps)));
    let mut moving = false;
    for t in 0..ticks {
        cpu.step();
        gpu.step();
        moving |= cpu.flow.max_speed() > 0;
        assert_eq!(cpu.hash(), gpu.hash(), "tick {t}: the world differs (diffusion and carrying on the GPU)");
        assert_eq!(cpu.world.tallies, gpu.world.tallies, "tick {t}: the edge tallies differ");
        assert_eq!((gpu.energy_gap(), gpu.mass_gap()), (0, 0), "tick {t}: books");
    }
    assert!(moving, "the air never moved, so carrying wasn't tested");
    if burning { assert!(gpu.world.cells.iter().any(|c| c.lit), "nothing burnt") }
}

#[test]
fn a_campfire_burns_the_same_with_diffusion_and_carrying_on_the_gpu() { compare_resident(burning_campfire, 60, true) }

#[test]
fn a_lit_valley_with_wind_is_the_same_with_carrying_on_the_gpu() {
    compare_resident(|| {
        let v = valley(7, 32, 32, 60);
        let (cx, cy) = (v.crib.0.min(28), v.crib.1.min(28));
        let gz = v.ground_at(cx, cy) as u16 + 1;
        let mut sim = Sim::new(v.world, Sky::afternoon());
        sim.flow.wind = [600, -250];
        for t in 0..40 { sim.schedule(Scheduled { tick: t, peer: 0, seq: 0, command: Command::Heat { x: cx as u16, y: cy as u16, z: gz, energy: joules(250) } }) }
        sim
    }, 40, false);
}

/// Diffusion, carrying and reaction all on the GPU, the state staying there
/// from one to the next: every tick the same as the CPU's.
fn compare_three(make: impl Fn() -> Sim, ticks: usize) { compare_three_on(make, ticks, true) }

fn compare_three_on(make: impl Fn() -> Sim, ticks: usize, burning: bool) { compare_phases(make, ticks, burning, false) }

/// Diffusion, the flow, carrying and reaction on the GPU (`with_flow`), or the three without the flow.
fn compare_phases(make: impl Fn() -> Sim, ticks: usize, burning: bool, with_flow: bool) { compare_phases_lazy(make, ticks, burning, with_flow, false) }

/// `lazy`: the state comes back only after the reaction (the four phases' hooks must all be set).
fn compare_phases_lazy(make: impl Fn() -> Sim, ticks: usize, burning: bool, with_flow: bool, lazy: bool) {
    let mut cpu = make();
    let mut gpu = make();
    let n = gpu.world.bricks();
    let g = match fv_gpu::pool::PoolDiffusion::with_mode(&gpu.world, fv_gpu::pool::shuffled(n, 11), fv_gpu::pool::Mode::Scratch, true) {
        Ok(mut g) => { g.lazy = lazy; Arc::new(Mutex::new(g)) }
        Err(e) => { no_gpu(e); return }
    };
    let (g2, g3, g4) = (g.clone(), g.clone(), g.clone());
    if with_flow { gpu.flow_hook = Some(Box::new(move |w, flow| g4.lock().unwrap().flow_step(w, flow))) }
    gpu.diffuse_hook = Some(Box::new(move |w, flow| g.lock().unwrap().diffuse(w, flow)));
    gpu.advect_hook = Some(Box::new(move |w, flow, steps| g2.lock().unwrap().advect(w, flow, steps)));
    gpu.react_hook = Some(Box::new(move |w, flow| g3.lock().unwrap().react(w, flow)));
    for t in 0..ticks {
        cpu.step();
        gpu.step();
        if cpu.hash() != gpu.hash() {
            let i = (0..cpu.world.cells.len()).find(|&i| cpu.world.cells[i] != gpu.world.cells[i]).unwrap();
            panic!("tick {t}: cell {i} differs\n cpu {:?}\n gpu {:?}", cpu.world.cells[i], gpu.world.cells[i]);
        }
        assert_eq!(cpu.flow, gpu.flow, "tick {t}: the flow differs");
        assert_eq!(cpu.world.tallies, gpu.world.tallies, "tick {t}: the edge tallies differ");
        assert_eq!((gpu.energy_gap(), gpu.mass_gap()), (0, 0), "tick {t}: books");
    }
    if burning { assert!(gpu.world.cells.iter().any(|c| c.lit), "nothing burnt") }
}

#[test]
fn a_campfire_burns_the_same_with_three_phases_on_the_gpu() { compare_three(burning_campfire, 60) }

/// A puddle on humid stone in sun and wind (open water: evaporation by the
/// wind over it, its own heat exchange), all three phases on the GPU.
fn puddle_in_wind() -> Sim {
    use fv_core::cell::Cell;
    use fv_core::substance::*;
    use fv_core::world::World;
    let air = celsius(20);
    let mut w = World::new(16, 16, 16, 9, air);
    let subs = w.subs.clone();
    let stone = Cell::solid_of(STONE, 1000, 2, air, &subs);
    for z in 0..4 { for y in 0..16 { for x in 0..16 { w.set(x, y, z, stone) } } }
    let humid = |c: &mut Cell| {
        let ml = c.gas_volume_ml(&subs);
        if ml <= 0 || c.gas_mass().0 == 0 { return }
        c.gas[Gas::Vapour as usize] = Mass(fv_core::tables::tables().saturated_at(air) * 85 / 100 * ml / CELL_VOLUME_ML);
        c.set_temperature(air, &subs);
    };
    for c in w.cells.iter_mut() { if c.substance == NONE { humid(c) } }
    let mut wet = Cell::puddle(5, air, &subs);
    humid(&mut wet);
    for y in 6..10 { for x in 6..10 { w.set(x, y, 4, wet) } }
    let mut sim = Sim::new(w, Sky::afternoon());
    sim.flow.wind_mean = [1_500, 400];
    sim
}

#[test]
fn a_puddle_in_wind_dries_the_same_with_three_phases_on_the_gpu() { compare_three_on(puddle_in_wind, 60, false) }

#[test]
fn a_lit_valley_is_the_same_with_three_phases_on_the_gpu() {
    compare_three_on(|| {
        let v = valley(7, 32, 32, 60);
        let (cx, cy) = (v.crib.0.min(28), v.crib.1.min(28));
        let gz = v.ground_at(cx, cy) as u16 + 1;
        let mut sim = Sim::new(v.world, Sky::afternoon());
        for t in 0..40 { sim.schedule(Scheduled { tick: t, peer: 0, seq: 0, command: Command::Heat { x: cx as u16, y: cy as u16, z: gz, energy: joules(250) } }) }
        sim
    }, 60, false);
}

#[test]
fn a_campfire_burns_the_same_with_the_flow_on_the_gpu_too() { compare_phases(burning_campfire, 60, true, true) }

#[test]
fn a_windy_puddle_is_the_same_with_the_flow_on_the_gpu_too() { compare_phases(puddle_in_wind, 40, false, true) }

/// The GPU's flame-reach fill reaches exactly the cells the CPU's does, with the same eddy size, every
/// tick of a burning campfire (and some do get reached).
#[test]
fn the_flame_reach_fill_is_the_same_on_the_gpu() {
    let mut sim = burning_campfire();
    let n = sim.world.bricks();
    let mut g = match fv_gpu::pool::PoolDiffusion::with_mode(&sim.world, fv_gpu::pool::shuffled(n, 13), fv_gpu::pool::Mode::Scratch, true) {
        Ok(g) => g,
        Err(e) => { no_gpu(e); return }
    };
    let mut reached = 0;
    for t in 0..60 {
        sim.step();
        let cpu = fv_core::chem::flame_reach(&sim.world);
        assert_eq!(g.flame_reach(&sim.world).iter().map(|&r| r > 0).collect::<Vec<_>>(), cpu, "tick {t}: flame reach differs");
        reached += cpu.iter().filter(|&&r| r).count();
    }
    assert!(reached > 0, "no flame reached any gas");
}

#[test]
fn a_campfire_burns_the_same_with_the_state_read_back_once_a_tick() { compare_phases_lazy(burning_campfire, 60, true, true, true) }

/// Each brick's hash worked out on the GPU equals World::brick_hash, after
/// the reaction of every tick of a burning campfire (shuffled slots).
#[test]
fn brick_hashes_are_the_same_on_the_gpu() {
    let mut sim = burning_campfire();
    let n = sim.world.bricks();
    let g = match fv_gpu::pool::PoolDiffusion::with_mode(&sim.world, fv_gpu::pool::shuffled(n, 17), fv_gpu::pool::Mode::Scratch, true) {
        Ok(g) => Arc::new(Mutex::new(g)),
        Err(e) => { no_gpu(e); return }
    };
    let checked = Arc::new(Mutex::new(0usize));
    let (g2, c2) = (g.clone(), checked.clone());
    sim.react_hook = Some(Box::new(move |w, flow| {
        let mut p = g2.lock().unwrap();
        p.react(w, flow);
        let gpu = p.brick_hashes(w);
        let cpu: Vec<u64> = (0..w.bricks()).map(|b| w.brick_hash(b)).collect();
        assert_eq!(gpu, cpu, "tick {}: brick hashes differ", w.tick);
        *c2.lock().unwrap() += 1;
    }));
    for _ in 0..30 { sim.step() }
    assert_eq!(*checked.lock().unwrap(), 30);
}

/// A 50 kW gas burner (as examples/burner.rs): a fire big enough that its
/// flame patch's D* is more than a cell, so the eddy sizes come from the
/// patch's own release.
fn burner_50kw() -> Sim {
    let (w, cells) = fv_core::scene::burner(24, 24);
    let mut sim = Sim::new(w, Sky::NIGHT);
    sim.flow.wind_mean = [0, 0];
    let per = (50e3 / ((16.0 - 0.58 * 2.257) * 1e6) * 0.05 * 1e9 / cells.len() as f64).round() as i64;
    let mut seq = 0;
    for t in 0..200 { for &(x, y, z) in &cells {
        seq += 1;
        sim.schedule(Scheduled { tick: t, peer: 0, seq, command: Command::Gas { x: x as u16, y: y as u16, z: z as u16, mass: Mass(per) } });
        if t < 40 { seq += 1; sim.schedule(Scheduled { tick: t, peer: 0, seq, command: Command::Heat { x: x as u16, y: y as u16, z: z as u16, energy: joules(200) } }) }
    }}
    sim
}

#[test]
fn a_burner_is_the_same_with_four_phases_on_the_gpu() { compare_phases_lazy(burner_50kw, 80, true, true, true) }
