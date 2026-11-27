//! Determinism, non-negotiable: identical hashes across runs, across the order minds are
//! stepped in, across thread counts (1, 2, 4, 8), and between the native and wasm32 builds.
//! And the hash must not be blind: a different seed, temperament or setting must change it.
//! `cargo test --release --test determinism -- --nocapture`

use fv_mind_lab::fox::scenarios::{self, DAY, SEED};
use fv_mind_lab::hashes::{pack, runs};
use fv_mind_lab::rng::hash;

/// Pinned: if behaviour changes, these move (and the change must be explained). Moved once
/// after review: the hash now covers settings, and temperament links are stored in steps of 4.
/// Moved again in stage 2: day learning rounds stochastically instead of truncating (behaviour:
/// calm, dim co-activation now learns), and the hash also covers the mind's place and entity
/// lists, where its own learning starts, and the nights it has slept.
/// Moved again in stage 3: comfort gated by need, a cost of distance and commitment to a goal
/// (behaviour), and the settings for them are hashed.
const PINNED: [u64; 6] = [0x6799_fcb8_4ffa_9a37, 0x6104_4744_1ced_2f65, 0x8ef4_96eb_49a3_2845, 0x4bd3_164a_c837_399f, 0x956a_7956_f529_2881, 0x4d35_b9f8_09cf_21e5];

#[test]
fn identical_runs_give_identical_pinned_hashes() {
    assert_eq!(hash(1, 2, 3, 4), 0x5a6b_0e97_f4a4_11b8, "the mixer itself is pinned");
    let (a, b) = (runs(1), runs(1));
    for ((n, x), (_, y)) in a.iter().zip(&b) { println!("{x:#018x} {y:#018x}  {n}"); }
    assert_eq!(a, b);
    let got: Vec<u64> = a.iter().map(|x| x.1).collect();
    assert_eq!(got, PINNED, "a pinned hash moved: behaviour changed");
}

#[test]
fn many_minds_same_hash_on_any_thread_count_and_any_order() {
    let n = 24;
    let ticks = DAY + 60;
    let ident: Vec<usize> = (0..n).collect();
    let rev: Vec<usize> = (0..n).rev().collect();
    let shuffled: Vec<usize> = { let mut v: Vec<(u64, usize)> = (0..n).map(|i| (hash(3, i as u64, 0, 1), i)).collect(); v.sort(); v.into_iter().map(|x| x.1).collect() };
    let reference = pack(SEED, n, ticks, 1, &ident);
    let mut rows = 0;
    for threads in [1, 2, 4, 8] {
        for (oname, order) in [("in order", &ident), ("reversed", &rev), ("shuffled", &shuffled)] {
            let h = pack(SEED, n, ticks, threads, order);
            println!("{n} foxes, one valley, {ticks} ticks, {threads} thread(s), minds {oname:9}: {h:#018x}");
            assert_eq!(h, reference, "{threads} threads, {oname}");
            rows += 1;
        }
    }
    println!("{rows} configurations, one hash");
}

#[test]
fn the_hash_is_not_blind() {
    let base = pack(SEED, 8, 200, 1, &(0..8).collect::<Vec<_>>());
    let other_seed = pack(SEED + 1, 8, 200, 1, &(0..8).collect::<Vec<_>>());
    let mut s1 = scenarios::dog_day(SEED); s1.run(200);
    let mut s2 = scenarios::dog_day(SEED); s2.m.cfg.sleep_keep = 3; s2.run(400);
    let mut s3 = scenarios::dog_day(SEED); s3.run(400);
    let mut s4 = scenarios::dog_charges(SEED, 3); s4.run(20);
    let mut s5 = scenarios::dog_charges(SEED, 4); s5.run(20);
    println!("seed {SEED} vs {}: {base:#x} vs {other_seed:#x}; sleep keeps 1/8 vs 1/3 after a night: {:#x} vs {:#x}; fox #3 vs #4: {:#x} vs {:#x}",
        SEED + 1, s3.state_hash(), s2.state_hash(), s4.state_hash(), s5.state_hash());
    assert_ne!(base, other_seed);
    assert_ne!(s2.state_hash(), s3.state_hash());
    assert_ne!(s4.state_hash(), s5.state_hash());
    // A single fast weight changed by one unit changes the hash.
    let mut m = s1.m.clone();
    let (k, f) = fv_mind_lab::store::Store::fast_entries(&m.store)[0];
    let h0 = m.state_hash();
    fv_mind_lab::store::Store::set_fast(&mut m.store, k, f + 1);
    assert_ne!(h0, m.state_hash());
}

/// The wasm32 build (run under node's WASI) must print the same hashes as the native build.
#[test]
fn native_and_wasm32_builds_agree() {
    let have_target = std::process::Command::new("rustup").args(["target", "list", "--installed"]).output().ok()
        .is_some_and(|o| String::from_utf8_lossy(&o.stdout).contains("wasm32-wasip1"));
    let have_node = std::process::Command::new("node").arg("--version").output().is_ok_and(|o| o.status.success());
    // Stage 3: a missing target or node is a failure, not a silent pass (stage 2's review found the
    // old skip passed). Skipping needs an explicit FV_SKIP_WASM=1, and says so.
    if !have_target || !have_node {
        let missing = format!("{}{}", if have_target { "" } else { "the wasm32-wasip1 target (rustup target add wasm32-wasip1) " }, if have_node { "" } else { "node (22+, for its built-in WASI)" });
        if std::env::var("FV_SKIP_WASM").is_ok_and(|v| v == "1") { println!("SKIPPED ON REQUEST (FV_SKIP_WASM=1): missing {missing}"); return; }
        panic!("cannot compare native and wasm32: missing {missing}. Install it, or set FV_SKIP_WASM=1 to skip on purpose.");
    }
    let dir = env!("CARGO_MANIFEST_DIR");
    let b = std::process::Command::new("cargo").current_dir(dir).args(["build", "--release", "--bin", "hashes", "--target", "wasm32-wasip1", "--target-dir", "target/wasm-test", "-q"]).status().unwrap();
    assert!(b.success(), "wasm32 build failed");
    let out = std::process::Command::new("node").current_dir(dir).args(["--no-warnings", "tools/wasi.mjs", "target/wasm-test/wasm32-wasip1/release/hashes.wasm"]).output().unwrap();
    assert!(out.status.success(), "node run failed: {}", String::from_utf8_lossy(&out.stderr));
    let wasm: Vec<u64> = String::from_utf8_lossy(&out.stdout).lines().map(|l| u64::from_str_radix(l.split_whitespace().next().unwrap().trim_start_matches("0x"), 16).unwrap()).collect();
    let native: Vec<u64> = runs(4).iter().chain(fv_mind_lab::hashes::people_runs(4).iter()).map(|x| x.1).collect();
    println!("native (4 threads for the pack) {native:x?}\nwasm32 under node (1 thread)    {wasm:x?}");
    assert_eq!(native, wasm);
}
