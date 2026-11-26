use fv_core::cell::Cell;
use fv_core::ledger::Books;
use fv_core::substance::*;
use fv_core::transport::{diffuse, Still, side};
use fv_core::units::*;
use fv_core::world::World;
fn main() {
    let mut w = World::new(24, 24, 24, 1, celsius(20));
    let s = side(&w.outside_air, &w.subs, w.ambient);
    println!("outside air: t={} excess={} gas={}", s.t.0, s.excess.0, w.outside_air.gas_mass().0);
    let stone = Cell::solid_of(STONE, 1000, 2, celsius(20), &w.subs);
    let ss = side(&stone, &w.subs, w.ambient);
    println!("stone: t={} excess={} gasml={}", ss.t.0, ss.excess.0, ss.gas_ml);
    for z in 4..20 { for y in 4..20 { for x in 4..20 {
        if x == 4 || x == 19 || y == 4 || y == 19 || z == 4 || z == 19 { w.set(x, y, z, stone) }
    }}}
    let hot = Cell::air(celsius(400), &w.subs);
    for z in 8..11 { for y in 10..13 { for x in 10..13 { w.set(x, y, z, hot) } } }
    let mut books = Books::open(&w);
    for tick in 0..300 {
        for t in w.tallies.iter_mut() { *t = Default::default() }
        diffuse(&mut w, &Still);
        for t in &w.tallies { books.record(t) }
        if tick % 30 == 0 { println!("{tick} edge_energy={} edge_gas={:?}", books.edge_energy, books.edge_gas) }
    }
    for x in [3usize, 4, 5, 10] { let c = w.get(x, 11, 9); println!("x={x} t={} gas={} sub={}", c.temperature(&w.subs).0, c.gas_mass().0, c.substance) }
    let c = w.get(0, 0, 23); println!("corner t={} gas={}", c.temperature(&w.subs).0, c.gas_mass().0);
}
