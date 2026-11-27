//! The energy and mass books must balance exactly, tick after tick.

use fv_core::cell::Cell;
use fv_core::ledger::{Books, Totals};
use fv_core::substance::*;
use fv_core::transport::{advect, diffuse, Still, Uniform};
use fv_core::units::*;
use fv_core::world::World;

fn record(books: &mut Books, w: &World) { for t in &w.tallies { books.record(t) } }

/// A stone room (walls 1 cell thick) inside a 24³ box, with a hot block of air.
fn sealed_room() -> World {
    let mut w = World::new(24, 24, 24, 1, celsius(20));
    let stone = Cell::solid_of(STONE, 1000, 2, celsius(20), &w.subs);
    for z in 4..20 { for y in 4..20 { for x in 4..20 {
        let wall = x == 4 || x == 19 || y == 4 || y == 19 || z == 4 || z == 19;
        if wall { w.set(x, y, z, stone) }
    }}}
    let hot = Cell::air(celsius(400), &w.subs);
    for z in 8..11 { for y in 10..13 { for x in 10..13 { w.set(x, y, z, hot) } } }
    w
}

#[test]
fn a_sealed_room_keeps_every_joule_and_milligram() {
    let mut w = sealed_room();
    let mut books = Books::open(&w);
    let wall_before = w.get(4, 11, 11).temperature(&w.subs);
    for tick in 0..300 {
        for t in w.tallies.iter_mut() { *t = Default::default() }
        diffuse(&mut w, &Still);
        record(&mut books, &w);
        let now = Totals::of(&w);
        assert_eq!(books.energy_gap(&now), 0, "energy gap at tick {tick}");
        assert_eq!(books.mass_gap(&now), 0, "mass gap at tick {tick}");
        assert_eq!(w.first_impossible(), None, "a store went negative at tick {tick}");
    }
    // Hot air expands: some gas seeps out through the stone's pores and
    // leaves the box, and the books account for every milligram of it.
    assert!(books.edge_gas.iter().sum::<i128>() <= 0);
    // The heat spread: the hot air cooled and pushed out, the walls warmed.
    assert!(w.get(11, 11, 9).temperature(&w.subs) < celsius(400));
    let _ = wall_before;
}

#[test]
fn wind_through_an_open_box_balances_with_what_crossed_the_edge() {
    let mut w = World::new(24, 16, 16, 2, celsius(20));
    let hot = Cell::air(celsius(300), &w.subs);
    for z in 4..8 { for y in 6..10 { for x in 4..8 { w.set(x, y, z, hot) } } }
    let grass = Cell::solid_of(GRASS, 1000, 1200, celsius(20), &w.subs);
    for y in 0..16 { for x in 0..24 { w.set(x, y, 0, grass) } }
    let mut books = Books::open(&w);
    let wind = Uniform([600, 100, 50]); // mm/s
    for tick in 0..200 {
        for t in w.tallies.iter_mut() { *t = Default::default() }
        advect(&mut w, &wind, 1);
        record(&mut books, &w);
        for t in w.tallies.iter_mut() { *t = Default::default() }
        diffuse(&mut w, &wind);
        record(&mut books, &w);
        let now = Totals::of(&w);
        assert_eq!(books.energy_gap(&now), 0, "energy gap at tick {tick}");
        assert_eq!(books.mass_gap(&now), 0, "mass gap at tick {tick}");
        assert_eq!(w.first_impossible(), None, "a store went negative at tick {tick}");
    }
    assert!(books.edge_energy != 0, "the warm air blew out of the box");
    assert!(books.edge_energy < 0, "more heat left than came in");
}

#[test]
fn threads_give_the_same_bits_as_one_thread() {
    let run = |threads: usize| {
        fv_core::world::THREADS.store(threads, core::sync::atomic::Ordering::Relaxed);
        let mut w = sealed_room();
        for _ in 0..60 { advect(&mut w, &Uniform([300, -200, 400]), 2); diffuse(&mut w, &Still) }
        w.hash()
    };
    let one = run(1);
    let many = run(0);
    fv_core::world::THREADS.store(0, core::sync::atomic::Ordering::Relaxed);
    assert_eq!(one, many);
}

#[test]
fn wind_through_a_sealed_room_never_overheats_a_drained_cell() {
    // Uniform wind through the sealed room with its hot block, carried in
    // one sub-step, with diffusion: the cells on the upwind walls drain to
    // almost no gas, and those on the downwind walls fill. Conduction
    // through a drained cell's air once moved far more heat than it held,
    // sending it below absolute zero and then to ~1 900 °C. With no heat
    // coming in, no cell may get hotter than the hottest air started (400
    // °C) or colder than the coldest (20 °C), whatever little gas it holds.
    let mut w = sealed_room();
    let mut books = Books::open(&w);
    let (cold, hot) = (celsius(20).0 - 1_000, celsius(400).0 + 1_000);
    for tick in 0..300 {
        for t in w.tallies.iter_mut() { *t = Default::default() }
        advect(&mut w, &Uniform([300, -200, 400]), 1);
        diffuse(&mut w, &Still);
        record(&mut books, &w);
        let now = Totals::of(&w);
        assert_eq!(books.energy_gap(&now), 0, "energy gap at tick {tick}");
        assert_eq!(books.mass_gap(&now), 0, "mass gap at tick {tick}");
        assert_eq!(w.first_impossible(), None, "a store went negative at tick {tick}");
        for (i, c) in w.cells.iter().enumerate() {
            if c.substance != NONE || c.gas_capacity().0 == 0 { continue }
            let t = c.gas_temperature().0;
            assert!((cold..=hot).contains(&t), "tick {tick}: cell {i}'s air at {:.0} °C with {} µg of gas, outside 20–400 °C", t as f64 / 1000.0 - 273.15, c.gas_mass().0);
        }
    }
}
