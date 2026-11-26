//! Radiation as a field (discrete ordinates): exact books, the same bits on
//! any number of threads, and light that takes the colour of what it bounced off.

use fv_core::cell::Cell;
use fv_core::rad::{radiate_by, Light, Radiation, Sky};
use fv_core::scene::crib_and_tree;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::substance::*;
use fv_core::units::*;
use fv_core::world::{World, THREADS};
use core::sync::atomic::Ordering::Relaxed;

fn burning_tree(threads: usize) -> (u64, Vec<[i64; 3]>, i128) {
    THREADS.store(threads, Relaxed);
    let (w, torch) = crib_and_tree(3, 24, 24, false);
    let mut sim = Sim::new(w, Sky::afternoon());
    sim.radiation = Radiation::Field;
    for t in 0..60 { for (k, p) in torch.iter().enumerate() {
        sim.schedule(Scheduled { tick: t, peer: 0, seq: k as u32, command: Command::Heat { x: p[0] as u16, y: p[1] as u16, z: p[2] as u16, energy: joules(400) } });
    }}
    let mut worst = 0i128;
    for _ in 0..120 { sim.step(); worst = worst.max(sim.energy_gap().abs()) }
    THREADS.store(0, Relaxed);
    (sim.hash(), sim.light.incident.clone(), worst)
}

#[test]
fn the_field_books_every_microjoule_and_threads_give_the_same_bits() {
    let (one, light_one, gap_one) = burning_tree(1);
    let (many, light_many, gap_many) = burning_tree(0);
    assert_eq!(gap_one, 0, "energy gap with one thread");
    assert_eq!(gap_many, 0, "energy gap with many threads");
    assert_eq!(one, many, "the world is the same on one thread and many");
    assert!(light_one == light_many, "and so is the light");
}

/// A stone wall standing on a floor of `floor`, in sunlight: the light
/// arriving at the wall's lower half, per band.
fn wall_light(floor: u8) -> [i64; 3] {
    let mut w = World::new(16, 16, 16, 1, celsius(15));
    let subs = w.subs.clone();
    let stone = Cell::solid_of(STONE, 1000, 2, celsius(15), &subs);
    let ground = Cell::solid_of(floor, 1000, 2, celsius(15), &subs);
    for y in 0..16 { for x in 0..16 { for z in 0..3 { w.set(x, y, z, ground) } } }
    for z in 3..10 { for x in 2..14 { w.set(x, 8, z, stone) } }
    let mut light = Light::new(w.cells.len());
    radiate_by(Radiation::Field, &mut w, &Sky::afternoon(), &mut light);
    let mut sum = [0i64; 3];
    for z in 3..6 { for x in 4..12 { let l = light.incident[w.idx(x, 8, z)]; for b in 0..3 { sum[b] += l[b] } } }
    sum
}

#[test]
fn light_takes_the_colour_of_what_it_bounced_off() {
    let over_stone = wall_light(STONE);
    let over_brick = wall_light(BRICK);
    let red_share = |l: [i64; 3]| l[0] as f64 / (l[0] + l[1] + l[2]) as f64;
    println!("red share {:.3} over brick, {:.3} over stone", red_share(over_brick), red_share(over_stone));
    assert!(red_share(over_brick) > red_share(over_stone) + 0.005,
            "a wall over red brick is lit redder: red share {:.3} over brick, {:.3} over stone", red_share(over_brick), red_share(over_stone));
}

#[test]
fn the_field_and_the_packets_agree_on_average() {
    // A hot iron block in a dark stone room: the heat each method puts into
    // the walls, the packets averaged over many ticks.
    let build = || {
        let mut w = World::new(16, 16, 16, 5, celsius(15));
        let subs = w.subs.clone();
        let stone = Cell::solid_of(STONE, 1000, 2, celsius(15), &subs);
        for z in 0..16 { for y in 0..16 { for x in 0..16 {
            if x == 0 || y == 0 || z == 0 || x == 15 || y == 15 || z == 15 { w.set(x, y, z, stone) }
        }}}
        let mut iron = Cell::solid_of(IRON, 1000, 0, celsius(15), &subs);
        iron.set_temperature(celsius(900), &subs);
        for z in 6..9 { for y in 6..9 { for x in 6..9 { w.set(x, y, z, iron) } } }
        w
    };
    let walls = |w: &World| -> i128 { w.cells.iter().filter(|c| c.substance == STONE).map(|c| c.thermal().0 as i128).sum() };
    let gain = |mode: Radiation, runs: u64| -> f64 {
        let mut total = 0f64;
        let mut light = Light::new(build().cells.len());
        for r in 0..runs {
            let mut w = build();
            w.tick = r;
            let before = walls(&w);
            radiate_by(mode, &mut w, &Sky::NIGHT, &mut light);
            total += (walls(&w) - before) as f64;
        }
        total / runs as f64
    };
    let packets = gain(Radiation::Packets, 40);
    let field = gain(Radiation::Field, 1);
    assert!(field > 0.0 && packets > 0.0);
    let ratio = field / packets;
    println!("walls gain {field:.0} µJ by the field, {packets:.0} µJ by packets (ratio {ratio:.3})");
    assert!((0.9..1.1).contains(&ratio), "walls gain {field:.0} µJ by the field, {packets:.0} µJ by packets (ratio {ratio:.3})");
}
