//! A village's every think, replayed on the GPU and compared with the CPU (3 days here; the
//! long runs are `cargo run --release --bin village-check -- 40`).
use fv_mind_lab::think2;
use fv_mind_lab::village::runs::{run, Setup};
use fv_mind_lab_gpu::{Gpu, Replay};
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn every_think_of_a_village_matches() {
    std::env::set_var("FV_THINK", "v2");
    let g: &'static Gpu = match Gpu::new() { Ok(g) => Box::leak(Box::new(g)), Err(e) => { if std::env::var_os("FV_GPU_REQUIRED").is_some() { panic!("{e}") } eprintln!("skipped: {e}"); return } };
    for (seed, every) in [(3u64, 1u32), (4, 4)] {
        let rp = Rc::new(RefCell::new(Replay::new(g)));
        let r2 = rp.clone();
        think2::record_start(2048, Box::new(move |b| r2.borrow_mut().batch(b)));
        let mut s = Setup::new(seed, 20, 3); s.think_every = every;
        run(&s, 1, None);
        think2::record_stop();
        let r = rp.borrow();
        eprintln!("seed {seed}, every {every}: {} thinks, {} mismatches", r.thinks, r.mismatches);
        assert!(r.thinks > 1000);
        assert_eq!(r.mismatches, 0, "{:?}", r.first_mismatch);
    }
}
