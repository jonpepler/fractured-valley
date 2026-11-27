//! One-command GPU benchmark: `cargo run --release -- [N] [M,..] [ticks] [chunk]`
//! Checks every tick's output and the final states against the CPU kernel, then times.
use mc::spread::Params;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let n: usize = a.get(1).map(|x| x.parse().unwrap()).unwrap_or(100_000);
    let ms: Vec<usize> = a.get(2).map(|x| x.split(',').map(|y| y.parse().unwrap()).collect()).unwrap_or(vec![1000]);
    let t: usize = a.get(3).map(|x| x.parse().unwrap()).unwrap_or(20);
    let chunk: usize = a.get(4).map(|x| x.parse().unwrap()).unwrap_or(4096);
    let g = fv_spread_kernels_gpu::Gpu::new().expect("no GPU adapter");
    println!("adapter: {} ({:?}, {:?})", g.info.name, g.info.device_type, g.info.backend);
    let p = Params::default();
    let sp = sk::harness::species(n, 7);
    for &m in &ms {
        let set = sk::bench::make_set(&sp, m, t, 1, if n >= 1_000_000 && m > 1000 { 10 } else { 1 }, 4);
        let r = fv_spread_kernels_gpu::run(&g, &set, &p, chunk, true);
        let (co, cs) = fv_spread_kernels_gpu::cpu(&set, &p);
        let same = r.outputs == co && r.states == cs;
        let mut best = f64::MAX;
        for _ in 0..3 { let x = fv_spread_kernels_gpu::run(&g, &set, &p, chunk, false); assert!(x.states == r.states, "GPU passes differ"); best = best.min(x.gpu_secs); }
        println!("N {n} M {m} T {t}: GPU matches CPU redef: {same} (overflowed instance-steps {}); GPU {:.3} us/step ({:.0} steps/s), host knots+packing {:.3} us/step",
            r.overflows, best * 1e6 / (m * t) as f64, (m * t) as f64 / best, r.pack_secs * 1e6 / (m * t) as f64);
        assert!(same, "GPU result differs from CPU");
    }
}
