//! Prints the pinned determinism hashes. Built natively and for wasm32 (run under node), the
//! output must be identical: `tools/wasm-hashes.sh`.
fn main() {
    let threads = if cfg!(target_arch = "wasm32") { 1 } else { 4 };
    for (name, h) in fv_mind_lab::hashes::runs(threads).into_iter().chain(fv_mind_lab::hashes::people_runs(threads)) { println!("{h:#018x}  {name}"); }
}
