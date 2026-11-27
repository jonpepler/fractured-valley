//! Memory: the CPU world's footprint, and what the GPU-resident layout
//! needs per active cell (from the shader's own strides).
use fv_core::cell::Cell;
use fv_core::rad::{Light, Sky};
use fv_core::scene::crib_and_tree;
use fv_core::sim::Sim;
use fv_gpu::*;

fn main() {
    let (w, _) = crib_and_tree(4, 48, 48, true);
    let sim = Sim::new(w, Sky::afternoon());
    let n = sim.world.cells.len();
    let cell = std::mem::size_of::<Cell>();
    let world = 2 * n * cell + sim.world.tallies.len() * std::mem::size_of::<fv_core::world::Tally>();
    let flow = sim.flow.vel.len() * 12;
    let light = Light::new(n);
    let light_b = (light.incident.len() + light.glow.len()) * 24 + light.dose.len() * 8;
    println!("cells {n}; Cell {cell} B; world cells+next {:.1} MB; flow {} blocks {:.2} MB; light {:.1} MB; CPU total {:.1} MB",
        world as f64 / 1e6, sim.flow.vel.len(), flow as f64 / 1e6, light_b as f64 / 1e6, (world + flow + light_b) as f64 / 1e6);
    let nl = sim.world.ny * sim.world.nz + sim.world.nx * sim.world.nz + sim.world.nx * sim.world.ny;
    let per = |words: usize| words * 4;
    println!("GPU diffusion buffers per cell (B): cells in {} + sides {} + velocities 12 + faces {} + limits 8 + out {} + edge {} (+ {} lower faces in all)",
        per(CELL_WORDS), per(SIDE_WORDS), per(3 * FLUX_WORDS), per(CELL_WORDS), per(EDGE_WORDS), nl);
}
