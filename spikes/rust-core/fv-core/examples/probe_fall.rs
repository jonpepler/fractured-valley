use fv_core::cell::{Cell, CHAR, VOLATILE};
use fv_core::scene::{bond_all, natural_bonds};
use fv_core::structure::settle;
use fv_core::substance::*;
use fv_core::units::*;
use fv_core::world::World;
fn main() {
    let mut w = World::new(24, 24, 24, 3, celsius(18));
    let subs = w.subs.clone();
    let stone = Cell::solid_of(STONE, 1000, 2, celsius(18), &subs);
    let wood = Cell::solid_of(WOOD, 1000, 120, celsius(18), &subs);
    let iron = Cell::solid_of(IRON, 1000, 0, celsius(18), &subs);
    for z in 0..4 { for y in 0..24 { for x in 0..24 { w.set(x, y, z, stone) } } }
    for z in 4..14 { for y in 11..13 { for x in 11..13 { w.set(x, y, z, wood) } } }
    for z in 14..17 { for y in 10..13 { for x in 11..14 { w.set(x, y, z, iron) } } }
    bond_all(&mut w, natural_bonds);
    for tick in 0..200 {
        for z in 4..6 { for y in 11..13 { for (x, rate) in [(11usize, 120i64), (12, 60)] {
            let c = w.get_mut(x, y, z); let gone = c.solid[VOLATILE].0 * rate / 1000; let cg = c.solid[CHAR].0 * rate / 2000; c.solid[VOLATILE].0 -= gone; c.solid[CHAR].0 -= cg;
        }}}
        if tick % 10 == 0 {
            let r = settle(&mut w);
            println!("{tick} worst={:.3} broke={} crushed={} loose={:?} int11={} int12={}", r.worst, r.broke.len(), r.crushed.len(), r.loose.iter().map(|g| g.len()).collect::<Vec<_>>(), w.get(11,11,4).integrity_permille(&w.subs), w.get(12,11,4).integrity_permille(&w.subs));
            if !r.loose.is_empty() { break }
        }
    }
}
