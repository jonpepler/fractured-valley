use fv_spread_kernels::{bench::*, harness::species};
use mc::spread::Params;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let n: usize = a[1].parse().unwrap();
    let ms: Vec<usize> = a[2].split(',').map(|x| x.parse().unwrap()).collect();
    let t: usize = a.get(3).map(|x| x.parse().unwrap()).unwrap_or(50);
    let p = Params::default();
    let sp = species(n, 7);
    for &m in &ms {
        let set = make_set(&sp, m, t, 1, 1, 4);
        let steps = (m * t) as f64;
        if m <= 10 {
            let o = best(3, || run_oracle(&sp, &set, &p, 1, 1, m));
            println!("oracle   N {n} M {m}: {:.2} us/step  hash {:016x}", o.secs * 1e6 / steps, o.hash);
        }
        for th in [1, 4] {
            let r = best(3, || run_exact(&set, &p, th, None));
            println!("exact t{th} N {n} M {m}: {:.2} us/step ({:.0} steps/s/core) visits/step {:.0} hash {:016x}", r.secs * 1e6 / steps, steps / r.secs / th as f64, r.visits as f64 / steps, r.hash);
        }
    }
}
