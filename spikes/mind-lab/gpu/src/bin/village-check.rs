//! Every think of a village, checked on the GPU: the village lives on the CPU with think v2, every
//! think is recorded (inputs, outputs, and its mind whenever it changed) and replayed on the GPU in
//! batches, and each GPU result is compared with the CPU's, state and workspace, bit for bit.
//! `cargo run --release --bin village-check -- [days 30] [people 30] [seed 1] [batch 4096] [think every 1]`
use fv_mind_lab::think2;
use fv_mind_lab::village::runs::{run, Setup};
use fv_mind_lab_gpu::{Gpu, Replay};
use std::cell::RefCell;
use std::rc::Rc;

fn main() {
    std::env::set_var("FV_THINK", "v2");
    let a: Vec<String> = std::env::args().collect();
    let arg = |i: usize, d: u64| a.get(i).and_then(|x| x.parse().ok()).unwrap_or(d);
    let (days, people, seed, batch, every) = (arg(1, 30) as u32, arg(2, 30) as usize, arg(3, 1), arg(4, 4096) as usize, arg(5, 1) as u32);
    let g: &'static Gpu = Box::leak(Box::new(Gpu::new().expect("a GPU adapter")));
    println!("adapter: {} ({:?}, {:?})", g.info.name, g.info.device_type, g.info.backend);
    let rp = Rc::new(RefCell::new(Replay::new(g)));
    let r2 = rp.clone();
    let t = std::time::Instant::now();
    think2::record_start(batch, Box::new(move |b| r2.borrow_mut().batch(b)));
    let mut setup = Setup::new(seed, people, days); setup.think_every = every;
    let (rec, _) = run(&setup, 1, None);
    think2::record_stop();
    let r = rp.borrow();
    println!("village: {people} people, {days} days, seed {seed}, every mind thinking every {every} ({} ticks, hash {:016x})", rec.ticks, rec.hash);
    println!("thinks compared: {}; mismatches: {}; saturation flags seen {:04b}", r.thinks, r.mismatches, r.flags_seen);
    println!("GPU: {} dispatches; {:.1} MB up (minds {:.1} MB), {:.1} MB down; {:.1} s in all ({:.1} s waiting on the GPU)",
        r.cost.dispatches, r.cost.bytes_up as f64 / 1e6, r.snap_bytes as f64 / 1e6, r.cost.bytes_down as f64 / 1e6, t.elapsed().as_secs_f64(), r.cost.run_s);
    if let Some(m) = &r.first_mismatch { println!("first mismatch: {m}"); }
    println!("GPU matches CPU: {}", r.mismatches == 0 && r.thinks > 0);
    if r.mismatches > 0 { std::process::exit(1); }
}
