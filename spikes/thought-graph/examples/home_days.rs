//! cargo run --release --example home_days [days]
use fv_thought_graph::experiments::{home_days, print_days};
use fv_thought_graph::scenarios::{HomeMove, SEED};
fn main() {
    let days: u32 = std::env::args().nth(1).and_then(|x| x.parse().ok()).unwrap_or(6);
    let only = std::env::args().nth(2);
    for v in [HomeMove::None, HomeMove::Near, HomeMove::Mid, HomeMove::Far, HomeMove::Removed, HomeMove::Second, HomeMove::Daily].into_iter().filter(|v| only.as_ref().map_or(true, |o| format!("{v:?}") == *o)) {
        let (_, d) = home_days(SEED, v, if v == HomeMove::Daily { days.max(14) } else if v == HomeMove::Near { days.max(12) } else { days });
        print_days(&format!("{v:?}"), &d);
    }
}
