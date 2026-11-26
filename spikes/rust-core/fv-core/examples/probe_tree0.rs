use fv_core::scene::crib_and_tree;
use fv_core::structure::settle;
fn main() {
    let (mut w, _) = crib_and_tree(4, 48, 48, false);
    let r = settle(&mut w);
    let mut by: std::collections::BTreeMap<u8, usize> = Default::default();
    for g in &r.loose { for p in g { *by.entry(w.get(p[0], p[1], p[2]).substance).or_default() += 1 } }
    println!("loose pieces {} cells by substance {:?} broke {} crushed {}", r.loose.len(), by, r.broke.len(), r.crushed.len());
}
