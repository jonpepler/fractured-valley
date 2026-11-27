//! The toolkit's determinism: one hash across runs, 1/2/4/8 threads and mind order, pinned for
//! the generated worlds, and the native and wasm32 builds agree (failing loudly if it can't run).
use fv_mind_lab::rng::StateHasher;
use fv_mind_lab::tools::{data, kit, study};
use std::hash::{Hash, Hasher};

/// Pinned: generated worlds only (the village rows move with the village's own code).
const PINS: [u64; 4] = [0x7bd9db872f317c22, 0x274e96956d3b91be, 0x7c1682010a2bcff5, 0xb55497f1d35647c8];

#[test]
fn pinned_and_repeatable() {
    let a = fv_mind_lab::tools::hashes();
    let b = fv_mind_lab::tools::hashes();
    for ((n, x), (_, y)) in a.iter().zip(&b) { println!("{x:#018x}  {n}"); assert_eq!(x, y, "{n} repeats"); }
    for (k, p) in PINS.iter().enumerate() { assert_eq!(a[k].1, *p, "pin moved: {}", a[k].0); }
}

/// Each observer's belt on its own; observers split over threads, in different orders.
fn makers(st: &data::Stream, threads: usize, order: &[usize]) -> u64 {
    let obs: Vec<usize> = order.iter().map(|&i| st.observers[i]).collect();
    let chunk = obs.len().div_ceil(threads);
    let mut out: Vec<(usize, u64)> = std::thread::scope(|s| {
        let hs: Vec<_> = obs.chunks(chunk).map(|c| { let mut sub = st.clone(); sub.observers = c.to_vec(); s.spawn(move || study::with_maker(&sub, kit::MAKER, 0, 400).into_iter().map(|r| { let mut h = StateHasher::new(3); r.hash(&mut h); (r.observer, h.finish()) }).collect::<Vec<_>>()) }).collect();
        hs.into_iter().flat_map(|h| h.join().unwrap()).collect()
    });
    out.sort();
    let mut h = StateHasher::new(9);
    for (o, x) in out { h.write_u64(o as u64); h.write_u64(x); }
    h.finish()
}

#[test]
fn threads_and_order_do_not_matter() {
    let st = data::routine(4, 10, 14, &[1, 2, 3, 4, 5, 6, 7, 8], 100);
    let n = st.observers.len();
    let fwd: Vec<usize> = (0..n).collect();
    let rev: Vec<usize> = (0..n).rev().collect();
    let shuf: Vec<usize> = { let mut v = fwd.clone(); v.sort_by_key(|&i| fv_mind_lab::rng::hash(5, i as u64, 0, 0)); v };
    let h0 = makers(&st, 1, &fwd);
    for t in [1, 2, 4, 8] { for o in [&fwd, &rev, &shuf] { assert_eq!(makers(&st, t, o), h0, "{t} threads"); } }
    let st2 = data::routine(5, 10, 14, &[1, 2, 3, 4, 5, 6, 7, 8], 100);
    assert_ne!(makers(&st2, 1, &fwd), h0, "the hash reacts to the seed");
    // A one-character change to a description changes its hash and shape.
    let a = kit::load(kit::TIMETABLE);
    let b = fv_mind_lab::tools::lang::parse(&kit::TIMETABLE.replace("(fade when 3)", "(fade when 2)")).unwrap();
    assert_ne!(a.digest(), b.digest()); assert_ne!(a.shape(), b.shape());
}

#[test]
fn native_and_wasm32_builds_agree() {
    let have_target = std::process::Command::new("rustup").args(["target", "list", "--installed"]).output().ok().is_some_and(|o| String::from_utf8_lossy(&o.stdout).contains("wasm32-wasip1"));
    let have_node = std::process::Command::new("node").arg("--version").output().is_ok_and(|o| o.status.success());
    if !have_target || !have_node {
        if std::env::var("FV_SKIP_WASM").is_ok_and(|v| v == "1") { println!("SKIPPED ON REQUEST (FV_SKIP_WASM=1)"); return; }
        panic!("cannot compare native and wasm32: missing the wasm32-wasip1 target or node. Install them, or set FV_SKIP_WASM=1 to skip on purpose.");
    }
    let dir = env!("CARGO_MANIFEST_DIR");
    let b = std::process::Command::new("cargo").current_dir(dir).args(["build", "--release", "--bin", "tool_hashes", "--target", "wasm32-wasip1", "--target-dir", "target/wasm-tools", "-q"]).status().unwrap();
    assert!(b.success(), "wasm32 build failed");
    let out = std::process::Command::new("node").current_dir(dir).args(["--no-warnings", "tools/wasi.mjs", "target/wasm-tools/wasm32-wasip1/release/tool_hashes.wasm"]).output().unwrap();
    assert!(out.status.success(), "node run failed: {}", String::from_utf8_lossy(&out.stderr));
    let wasm: Vec<u64> = String::from_utf8_lossy(&out.stdout).lines().map(|l| u64::from_str_radix(l.split_whitespace().next().unwrap().trim_start_matches("0x"), 16).unwrap()).collect();
    let native: Vec<u64> = fv_mind_lab::tools::hashes().iter().map(|x| x.1).collect();
    println!("native {native:x?}\nwasm32 {wasm:x?}");
    assert_eq!(native, wasm);
}
