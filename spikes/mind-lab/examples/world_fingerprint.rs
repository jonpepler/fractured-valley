//! world-only hashes of the pinned teaching and danger runs (behaviour fingerprint).
use fv_mind_lab::people::person::Medium;
use fv_mind_lab::people::scenarios::{Setup, Spring, BRYN, DAY, DEE};
use fv_mind_lab::people::world::Event;
use fv_mind_lab::danger::runs::*;
fn main() {
    let order: Vec<usize> = (0..4).collect();
    let drive = |mut v: fv_mind_lab::people::sim::Valley, ticks: u32| { for _ in 0..ticks { v.step(1, &order); } (v.w.state_hash(), v.w.log.len()) };
    println!("{:x?}", drive(Setup::new(1).temper(1).build(), DAY));
    let mut s = Setup::new(2).temper(0); s.spring = Spring::DriesAt(5);
    println!("{:x?}", { let mut v = s.build(); let (x, y) = v.w.spring(); v.at(DAY + 20, Event::Put { who: 0, x: x + 2, y: y + 2 }); drive(v, 3 * DAY) });
    let mut s = Setup::new(3).temper(2); s.pos[DEE] = (17, 25); s.knows[DEE].retain(|&x| x != fv_mind_lab::people::world::STREAM);
    println!("{:x?}", drive(s.build(), 2 * DAY));
    let mut s = Setup::new(4).temper(1); s.medium = Medium::Note; s.days = 5;
    println!("{:x?}", { let mut v = s.build(); v.at(2 * DAY + 50, Event::LoseNotes { who: BRYN }); drive(v, 5 * DAY) });
    let mut s = Setup::new(5).temper(0); s.medium = Medium::Map;
    println!("{:x?}", drive(s.build(), DAY));
    let d = |mut v: fv_mind_lab::people::sim::Valley, seed: u64| { meet(&mut v, seed, 8); for _ in 0..WATCH { v.step(1, &order); } (v.w.state_hash(), v.w.log.len()) };
    for seed in 1..=6 { for t in 0..4 { for (al, ex) in [(false,false),(true,true),(true,false),(false,true)] {
        print!("{:x} ", d(build(&Case { seed, temper: TEMPERS[t].1, allies: al, expected: ex }, 700, 2).0, seed).0);
    }}}
    println!();
    println!("{:x?}", d(build_cast(&Case { seed: 3, temper: TEMPERS[2].1, allies: true, expected: false }, &Cast { ally_dist: 16, calls: 700, ..Cast::default() }).0, 3));
}
