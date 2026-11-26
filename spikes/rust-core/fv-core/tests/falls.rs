//! A post burnt through at its foot: the post and its iron block break off,
//! lean, fall and come to rest, and every joule is accounted for on the way:
//! gravity's work becomes motion, then heat.

use fv_core::cell::{Cell, CHAR, VOLATILE};
use fv_core::rad::Sky;
use fv_core::scene::{bond_all, natural_bonds};
use fv_core::sim::Sim;
use fv_core::substance::*;
use fv_core::units::*;
use fv_core::world::World;

fn scene() -> World {
    let mut w = World::new(40, 40, 24, 3, celsius(18));
    let subs = w.subs.clone();
    let stone = Cell::solid_of(STONE, 1000, 2, celsius(18), &subs);
    let wood = Cell::solid_of(WOOD, 1000, 120, celsius(18), &subs);
    let iron = Cell::solid_of(IRON, 1000, 0, celsius(18), &subs);
    for z in 0..4 { for y in 0..40 { for x in 0..40 { w.set(x, y, z, stone) } } }
    for z in 4..14 { for y in 19..21 { for x in 19..21 { w.set(x, y, z, wood) } } }
    // The block overhangs on +x, so when the post goes it leans that way.
    for z in 14..17 { for y in 18..21 { for x in 19..22 { w.set(x, y, z, iron) } } }
    bond_all(&mut w, natural_bonds);
    w
}

#[test]
fn a_post_burnt_through_falls_with_its_block_and_the_books_balance() {
    let mut sim = Sim::new(scene(), Sky::NIGHT);
    let iron_top_before = 16;
    let mut fell = false;
    let mut settled_after_fall = false;
    for tick in 0..800 {
        // Stand-in for a fire at the post's foot (which would take most of
        // an hour): char and burn away the bottom of the post, one side first.
        if tick < 200 {
            for z in 4..6 { for y in 19..21 { for (x, rate) in [(19usize, 120i64), (20, 60)] {
                let c = sim.world.get_mut(x, y, z);
                let gone = c.solid[VOLATILE].0 * rate / 1000;
                let char_gone = c.solid[CHAR].0 * rate / 2000;
                // Matter leaving the box this way is booked as leaving it.
                let subs = sim.world.subs.clone();
                let before = sim.world.get(x, y, z).chemical(&subs).0 + sim.world.get(x, y, z).solid_mass().0;
                let c = sim.world.get_mut(x, y, z);
                // The matter takes its share of the solid's heat with it.
                let solid = c.solid[0].0 + c.solid[1].0 + c.solid[2].0;
                let heat = (c.skin.0 as i128 * (gone + char_gone) as i128 / solid.max(1) as i128) as i64
                    + (c.solid_energy.0 as i128 * (gone + char_gone) as i128 / solid.max(1) as i128) as i64;
                c.skin.0 -= (c.skin.0 as i128 * (gone + char_gone) as i128 / solid.max(1) as i128) as i64;
                c.solid_energy.0 -= (c.solid_energy.0 as i128 * (gone + char_gone) as i128 / solid.max(1) as i128) as i64;
                c.solid[VOLATILE].0 -= gone;
                c.solid[CHAR].0 -= char_gone;
                sim.books.commands -= heat as i128;
                let after = sim.world.get(x, y, z).chemical(&subs).0 + sim.world.get(x, y, z).solid_mass().0;
                let _ = (before, after);
                sim.books.commands -= (gone * 15_600 + char_gone * 32_800) as i128;
                sim.books.commands_mass -= (gone + char_gone) as i128;
            }}}
        }
        sim.step();
        assert_eq!(sim.energy_gap(), 0, "energy gap at tick {tick} ({} pieces in flight)", sim.bodies.pieces.len());
        if let Some((x, y, z)) = sim.world.first_impossible() { panic!("a store went negative at tick {tick} at ({x},{y},{z}): {:?}", sim.world.get(x, y, z)) }
        assert_eq!(sim.mass_gap(), 0, "mass gap at tick {tick}");
        // Wait for the block itself to fall (charred crumbs may drop first).
        let iron_flying = sim.bodies.cells().any(|(_, _, c)| c.substance == IRON);
        if iron_flying { fell = true }
        if fell && !sim.bodies.in_flight() { settled_after_fall = true; break }
        if std::env::var("FV_TRACE").is_ok() && tick % 50 == 0 {
            for p in &sim.bodies.pieces { let b = &sim.bodies.physics.bodies[p.handle]; eprintln!("{tick} piece {} cells at {:?} v={:.2} w={:.2}", p.cells.len(), b.translation(), b.linvel().length(), b.angvel().length()) }
        }
    }
    assert!(fell, "the post and block came loose and fell");
    assert!(settled_after_fall, "and came to rest");
    assert!(sim.books.gravity_work > 0, "gravity did work on the way down: {} µJ", sim.books.gravity_work);
    let iron_top = (0..24).rev().find(|&z| (0..40).any(|y| (0..40).any(|x| sim.world.get(x, y, z).substance == IRON))).expect("the block is still in the box");
    assert!(iron_top < iron_top_before, "the block ended lower (top at {iron_top})");
    let iron_cells = sim.world.cells.iter().filter(|c| c.substance == IRON).count();
    assert_eq!(iron_cells, 27, "all of the block is back in the world");
}
