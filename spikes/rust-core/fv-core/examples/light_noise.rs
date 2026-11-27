//! How noisy is sunlight on open grass? The dead-tree scene's grass (z = 6),
//! in a 16×16 patch well away from the tree's shadow, under the afternoon
//! sun: the light each cell takes each tick (red + green + blue, µJ), and its
//! spread across the patch (coefficient of variation), averaged over ticks.
//!
//!     cargo run --release -p fv-core --example light_noise
use fv_core::rad::Sky;
use fv_core::scene::crib_and_tree;
use fv_core::sim::Sim;

fn main() {
    let ticks: usize = std::env::var("TICKS").ok().and_then(|v| v.parse().ok()).unwrap_or(30);
    let (w, _) = crib_and_tree(4, 48, 48, true);
    let mut sim = Sim::new(w, Sky::afternoon());
    let (mut cv_sum, mut mean_sum, mut total_cv_cells) = (0.0, 0.0, vec![0f64; 256]);
    for _ in 0..ticks {
        sim.step();
        let w = &sim.world;
        let vals: Vec<f64> = (0..256).map(|k| {
            let (x, y) = (2 + k % 16, 2 + k / 16);
            let i = w.idx(x, y, 6);
            sim.light.incident[i].iter().sum::<i64>() as f64
        }).collect();
        for (t, v) in total_cv_cells.iter_mut().zip(&vals) { *t += v }
        let mean = vals.iter().sum::<f64>() / vals.len() as f64;
        let sd = (vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / vals.len() as f64).sqrt();
        cv_sum += sd / mean.max(1.0);
        mean_sum += mean;
    }
    let m = total_cv_cells.iter().sum::<f64>() / 256.0;
    let sd = (total_cv_cells.iter().map(|v| (v - m).powi(2)).sum::<f64>() / 256.0).sqrt();
    println!("open grass, {ticks} ticks: light per cell per tick {:.0} µJ; spread across cells per tick {:.1} %; spread of the {ticks}-tick totals {:.1} %",
        mean_sum / ticks as f64, 100.0 * cv_sum / ticks as f64, 100.0 * sd / m.max(1.0));
    assert_eq!(sim.energy_gap(), 0);
}
