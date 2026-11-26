use fv_core::rad::Sky;
use fv_core::scene::valley;
use fv_core::sim::Sim;
fn main() {
    let v = valley(3, 96, 64, 120);
    println!("cells={} post={:?} tree={:?} crib={:?}", v.world.cells.len(), v.post, v.tree, v.crib);
    let mut sim = Sim::new(v.world, Sky::afternoon());
    let t0 = std::time::Instant::now();
    for _ in 0..20 { sim.step() }
    println!("{:.0} ms/tick, gap {} {}", t0.elapsed().as_millis() as f64 / 20.0, sim.energy_gap(), sim.mass_gap());
}
