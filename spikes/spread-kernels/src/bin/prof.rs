use fv_spread_kernels::{bench::*, harness::species};
use mc::spread::Params;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let n: usize = a[1].parse().unwrap(); let m: usize = a[2].parse().unwrap(); let t: usize = a[3].parse().unwrap();
    let which = a.get(4).cloned().unwrap_or("exact-pruned".into());
    let th: usize = a.get(5).map(|x| x.parse().unwrap()).unwrap_or(1);
    let p = Params::default();
    let sp = species(n, 7);
    let set = make_set(&sp, m, t, 1, 1, 4);
    let r = best(5, || run_variant(&which, &set, &p, th, None));
    println!("{which} t{th}: {:.2} us/step hash {:016x}", r.secs * 1e6 / (m * t) as f64, r.hash);
}
