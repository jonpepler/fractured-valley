//! Prints the toolkit's determinism hashes. Built natively and for wasm32 (run under node), the
//! output must be identical (`tests/tools_determinism.rs`).



fn main() {
    if std::env::args().nth(1).as_deref() == Some("debug") { debug(); return; }
    for (name, h) in fv_mind_lab::tools::hashes() { println!("{h:#018x}  {name}"); }


}

#[allow(dead_code)]
fn debug() {
    use fv_mind_lab::tools::{data, kit, study};
    let st = data::routine(2, 8, 16, &[1, 2], 100);
    for r in study::with_maker(&st, kit::MAKER, 0, 400) { for m in &r.made { println!("{} {} {:?}", m.0, m.2, m.1); } println!("score {:x} belt {:x} thought {}", r.score.hash, r.belt_hash, r.thought); }
}
