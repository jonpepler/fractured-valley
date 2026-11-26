use fv_core::cell::{Cell, CHAR, VOLATILE};
use fv_core::scene::{bond_all, natural_bonds};
use fv_core::structure::{bond, settle};
use fv_core::substance::*;
use fv_core::units::*;
use fv_core::world::World;
fn main() {
    let mut w = World::new(16, 16, 24, 1, celsius(18));
    let subs = w.subs.clone();
    let stone = Cell::solid_of(STONE, 1000, 2, celsius(18), &subs);
    let wood = Cell::solid_of(WOOD, 1000, 120, celsius(18), &subs);
    let iron = Cell::solid_of(IRON, 1000, 0, celsius(18), &subs);
    for z in 0..4 { for y in 0..16 { for x in 0..16 { w.set(x, y, z, stone) } } }
    for z in 4..14 { for y in 7..9 { for x in 7..9 { w.set(x, y, z, wood) } } }
    for z in 14..17 { for y in 6..9 { for x in 7..10 { w.set(x, y, z, iron) } } }
    bond_all(&mut w, natural_bonds);
    for step in 0..300 {
        for z in 4..7 { for y in 7..9 { for (x, rate) in [(7usize, 100i64), (8, 50)] {
            let c = w.get_mut(x, y, z); let gone = c.solid[VOLATILE].0 * rate / 1000; c.solid[VOLATILE].0 -= gone; c.solid[CHAR].0 += gone / 5 - c.solid[CHAR].0 * rate / 2000;
        }}}
        let r = settle(&mut w);
        if step % 30 == 0 || !r.broke.is_empty() {
            let b7 = bond(w.get(7, 7, 5), w.get(7, 7, 6), &w.subs);
            println!("{step} worst={:.4} broke={} loose={} int7={} int8={} cap7 t={:.0}N c={:.0}N", r.worst, r.broke.len(), r.loose.len(), w.get(7,7,5).integrity_permille(&w.subs), w.get(8,7,5).integrity_permille(&w.subs), b7.tension, b7.compression);
        }
        if !r.loose.is_empty() { for g in &r.loose { let subs: std::collections::BTreeSet<u8> = g.iter().map(|p| w.get(p[0],p[1],p[2]).substance).collect(); let zs: Vec<usize> = g.iter().map(|p| p[2]).collect(); println!("loose {} cells subs {:?} z {}..{}", g.len(), subs, zs.iter().min().unwrap(), zs.iter().max().unwrap()); } println!("crushed {:?} broke {:?}", r.crushed, r.broke); break }
    }
}
