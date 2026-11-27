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
