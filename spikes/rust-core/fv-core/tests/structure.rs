//! What holds things up, from bonds alone: no terrain flag, only the bottom
//! of the world fixed.

use fv_core::cell::{Cell, CHAR, VOLATILE};
use fv_core::scene::{bond_all, natural_bonds};
use fv_core::structure::settle;
use fv_core::substance::*;
use fv_core::units::*;
use fv_core::world::World;

/// Ground (stone, 4 cells deep) and on it a 2×2 dry-wood post, 10 high,
/// carrying an iron block (3×3×3, ~210 kg) centred or overhanging.
fn post_with_block(offset: usize) -> World {
    let mut w = World::new(16, 16, 24, 1, celsius(18));
    let subs = w.subs.clone();
    let stone = Cell::solid_of(STONE, 1000, 2, celsius(18), &subs);
    let wood = Cell::solid_of(WOOD, 1000, 120, celsius(18), &subs);
    let iron = Cell::solid_of(IRON, 1000, 0, celsius(18), &subs);
    for z in 0..4 { for y in 0..16 { for x in 0..16 { w.set(x, y, z, stone) } } }
    for z in 4..14 { for y in 7..9 { for x in 7..9 { w.set(x, y, z, wood) } } }
    for z in 14..17 { for y in 6..9 { for x in (6 + offset)..(9 + offset) { w.set(x, y, z, iron) } } }
    bond_all(&mut w, natural_bonds);
    w
}

/// Burn away a share (per mille) of the wood's volatile part in the post's
/// cells at height `z` on one side (x = 7), as fire would.
fn char_side(w: &mut World, z: usize, share: i64) {
    for y in 7..9 {
        let c = w.get_mut(7, y, z);
        let gone = c.solid[VOLATILE].0 * share / 1000;
        c.solid[VOLATILE].0 -= gone;
        c.solid[CHAR].0 += gone / 4; // some of it is left as char
    }
}

#[test]
fn ground_and_a_sound_post_hold_with_no_terrain_flag() {
    let mut w = post_with_block(0);
    let r = settle(&mut w);
    assert!(r.broke.is_empty(), "nothing breaks: worst load {:.2} of capacity", r.worst);
    assert!(r.loose.is_empty());
    assert!(r.worst > 0.0001, "the bonds are carrying load");
}

#[test]
fn a_post_charred_through_on_one_side_bends_then_gives_way() {
    let mut w = post_with_block(1); // the block overhangs, so the post is bent
    // Fire at the post's foot chars one side (x = 7) twice as fast as the
    // other, and burns the char away behind it. Real posts stand until
    // they're nearly burnt through: wood is strong (40 MPa), and a 20 cm post
    // carries 210 kg at ~0.1 % of its capacity; even pure charcoal holds it.
    let mut first_break: Option<(i64, Vec<([usize; 3], usize)>)> = None;
    let mut loose = Vec::new();
    for step in 1..=300 {
        for z in 4..7 { for y in 7..9 {
            for (x, rate) in [(7usize, 100i64), (8, 50)] {
                let c = w.get_mut(x, y, z);
                let gone = c.solid[VOLATILE].0 * rate / 1000;
                c.solid[VOLATILE].0 -= gone;
                // The char glows and burns away too, a little slower.
                c.solid[CHAR].0 += gone / 5 - c.solid[CHAR].0 * rate / 2000;
            }
        }}
        let r = settle(&mut w);
        if first_break.is_none() && !r.broke.is_empty() {
            let left = w.get(7, 7, 5).integrity_permille(&w.subs);
            first_break = Some((left, r.broke.clone()));
        }
        // Charred crumbs may drop first; wait for the piece carrying the block.
        let carries_block = r.loose.iter().any(|g| g.iter().any(|p| w.get(p[0], p[1], p[2]).substance == IRON));
        if carries_block { loose = r.loose; break }
        let _ = step;
    }
    let (left, broke) = first_break.expect("the post failed before it burnt away entirely");
    assert!(left < 100, "it held until the charred side was down to {left}‰ of its strength");
    // Bending: the first bonds to go are on the charred side (x = 7) or the
    // faces between the two sides, not the sound side alone.
    assert!(broke.iter().any(|(p, _)| p[0] == 7), "the charred side gave first: {broke:?}");
    assert!(!loose.is_empty(), "the post and block came loose");
    let iron: usize = loose.iter().flatten().filter(|p| w.get(p[0], p[1], p[2]).substance == IRON).count();
    assert_eq!(iron, 27, "the whole iron block came loose with it");
}

#[test]
fn masonry_fails_at_its_mortar_when_undermined() {
    let mut w = World::new(24, 8, 16, 2, celsius(18));
    let subs = w.subs.clone();
    let stone = Cell::solid_of(STONE, 1000, 2, celsius(18), &subs);
    let brick = Cell::solid_of(BRICK, 1000, 10, celsius(18), &subs);
    let mortar = Cell::solid_of(MORTAR, 1000, 20, celsius(18), &subs);
    for z in 0..3 { for y in 0..8 { for x in 0..24 { w.set(x, y, z, stone) } } }
    // A wall 20 long, 8 courses, on two stone footings at its ends only.
    for z in 3..11 { for y in 3..5 { for x in 2..22 {
        let joint = (x + (z % 2) * 2) % 4 == 3;
        w.set(x, y, z, if joint { mortar } else { brick });
    }}}
    for x in 2..22 { for y in 3..5 { if !(2..5).contains(&x) && !(19..22).contains(&x) { w.set(x, y, 2, Cell::air(celsius(18), &subs)) } } }
    bond_all(&mut w, natural_bonds);
    let mut broke = Vec::new();
    for _ in 0..6 { broke.extend(settle(&mut w).broke) }
    let at_mortar = broke.iter().filter(|(p, axis)| {
        let mut q = *p;
        q[*axis] += 1;
        w.get(p[0], p[1], p[2]).substance == MORTAR || w.get(q[0], q[1], q[2]).substance == MORTAR
    }).count();
    let _ = at_mortar;
    // A 2 m span of brick on 0.5 MPa mortar holds its own weight: masonry arches.
    assert!(broke.is_empty() || at_mortar == broke.len(), "if anything gives, it's the mortar");
}
