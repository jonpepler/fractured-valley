//! The GPU kernel equals the CPU redefined kernel (every tick's output and the final states),
//! on whatever adapter is present; skipped when there is none. On this machine: llvmpipe via
//! the GL backend (`WGPU_BACKEND=gl`); lavapipe's Vulkan driver crashes in its shader JIT.
use mc::spread::Params;
#[test]
fn gpu_matches_cpu_redef() {
    let Some(g) = fv_spread_kernels_gpu::Gpu::new() else { eprintln!("no adapter: skipped"); return; };
    let p = Params::default();
    for (n, m, chunk) in [(5000usize, 70usize, 64usize), (20_000, 200, 4096)] {
        let sp = sk::harness::species(n, 3);
        let set = sk::bench::make_set(&sp, m, 30, 1, 1, 4);
        let r = fv_spread_kernels_gpu::run(&g, &set, &p, chunk, true);
        let (o, s) = fv_spread_kernels_gpu::cpu(&set, &p);
        assert_eq!(r.overflows, 0);
        assert!(r.outputs == o, "outputs n {n}");
        assert!(r.states == s, "states n {n}");
        let r2 = fv_spread_kernels_gpu::run(&g, &set, &p, chunk / 2 + 1, false);
        assert!(r2.states == s, "chunking changes results");
    }
}
