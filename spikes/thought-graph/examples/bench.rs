use fv_thought_graph::scale::bench;
fn main() {
    for (n, gt, ht, cmp) in [(1_000, 5000, 300, 200), (10_000, 5000, 40, 40), (100_000, 3000, 3, 6)] {
        let r = bench(n, 2048, gt, ht, cmp, 42);
        println!("{r:?}");
    }
}
