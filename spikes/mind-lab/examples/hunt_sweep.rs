//! Quick sweep of the planned and solo hunts over seeds and temperament sets (one line each).
use fv_mind_lab::hunt::runs::*;
fn main() {
    for mode in [Mode::Planned, Mode::Solo] { for set in 0..3 {
        let mut line = format!("{mode:?} {:>8}: ", SETS[set]);
        let mut n = 0;
        for seed in 1..=12 { let (o, _) = hunt(&Case { seed, set, mode }); if o.caught.is_some() { n += 1; }
            line += &format!("{} ", match o.caught { Some((t, w)) => format!("{t}/{w}"), None => format!("{}({}j{}d{}s r{})", if o.escaped.is_some() { "E" } else { "-" }, o.joined, o.declined, o.silent, o.deer_ran) }); }
        println!("{line}  caught {n}/12");
    } }
}
