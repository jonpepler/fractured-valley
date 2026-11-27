//! Writes one diffusion phase (the dead tree at tick AT, default 30) for the
//! browser test: the shader, its packed inputs and the CPU's result in the
//! shader's output layout. `cargo run --release -p fv-gpu --example dump_phase -- DIR`
use fv_core::rad::Sky;
use fv_core::scene::crib_and_tree;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::substance::GASES;
use fv_core::units::*;

fn main() {
    let dir = std::env::args().nth(1).expect("output directory");
    let at: u64 = std::env::var("AT").ok().and_then(|v| v.parse().ok()).unwrap_or(30);
    let (w, torch) = crib_and_tree(4, 48, 48, true);
    let mut sim = Sim::new(w, Sky::afternoon());
    for t in 0..200 { for (i, p) in torch.iter().enumerate().filter(|(i, _)| i % 3 == 0) { sim.schedule(Scheduled { tick: t, peer: 0, seq: i as u32, command: Command::Heat { x: p[0] as u16, y: p[1] as u16, z: p[2] as u16, energy: joules(500) } }) } }
    for _ in 0..at { sim.step() }
    let w0 = sim.world.clone();
    let inp = fv_gpu::inputs(&w0, &sim.flow);
    let mut w = w0.clone();
    fv_core::transport::diffuse(&mut w, &sim.flow);
    // The expected output: each cell's stores, then each brick's tally change.
    let mut exp = Vec::new();
    for c in &w.cells { fv_gpu::pack_cell(&mut exp, c) }
    let p64 = |v: &mut Vec<u32>, x: i64| { v.push(x as u32); v.push((x as u64 >> 32) as u32) };
    for b in 0..w.bricks() {
        p64(&mut exp, w.tallies[b].energy_in - w0.tallies[b].energy_in);
        for k in 0..GASES { p64(&mut exp, w.tallies[b].gas_in[k] - w0.tallies[b].gas_in[k]) }
    }
    let wr = |name: &str, d: &[u8]| std::fs::write(format!("{dir}/{name}"), d).unwrap();
    wr("diffuse.wgsl", fv_gpu::shader_source().as_bytes());
    wr("params.bin", bytemuck::cast_slice(&inp.params));
    wr("cells.bin", bytemuck::cast_slice(&inp.cells));
    wr("sides.bin", bytemuck::cast_slice(&inp.sides));
    wr("vel.bin", bytemuck::cast_slice(&inp.vel));
    wr("expected.bin", bytemuck::cast_slice(&exp));
    println!("tick {at}: {} cells, {} bricks written to {dir}", w.cells.len(), w.bricks());
}
