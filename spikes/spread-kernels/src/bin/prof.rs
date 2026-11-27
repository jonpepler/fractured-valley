use fv_spread_kernels::{bench::*, harness::species};
use mc::spread::Params;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let n: usize = a[1].parse().unwrap(); let m: usize = a[2].parse().unwrap(); let t: usize = a[3].parse().unwrap();
    let which = a.get(4).cloned().unwrap_or("exact".into());
    let p = Params::default();
    let sp = species(n, 7);
    let set = make_set(&sp, m, t, 1, 1, 1);
    let f: StepFn = match which.as_str() { "nodiv" => fv_spread_kernels::exact::step_cfg::<false>, _ => fv_spread_kernels::exact::step };
    let r = if which == "fast" { best(5, || run_fast(&set, &p, 1, None)) } else { best(5, || run_kernel(&set, &p, 1, None, f)) };
    println!("{:.2} us/step hash {:016x}", r.secs * 1e6 / (m * t) as f64, r.hash);
}
