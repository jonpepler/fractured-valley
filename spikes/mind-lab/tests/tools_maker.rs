//! The tool-maker: different travellers invent different route tools, a blocked way is learnt
//! from at once, and a frightened traveller falls back to the cheap version of its map.
use fv_mind_lab::tools::routes::*;

#[test]
fn commuters_keep_journeys_and_roamers_make_maps() {
    for seed in 1..=4 {
        let w = world(seed, &commuter_goals(seed));
        for (who, habit) in cast() {
            let r = travel(seed, who, habit, 18, Variant::Maker, &w);
            let last = r.made.last().map(|m| m.1.clone()).unwrap_or_default();
            println!("seed {seed} {who} {habit:?}: {}", r.made.iter().map(|m| format!("night {} {}", m.0, m.1)).collect::<Vec<_>>().join("; "));
            assert!(!r.made.is_empty(), "every traveller makes a route tool");
            match habit { Habit::Roamer => assert!(last.starts_with("made(map"), "a roamer ends with a map: {last}"), Habit::Commuter => {} }
        }
    }
}

#[test]
fn revision_and_fear() {
    let seed = 1;
    let w = world(seed, &commuter_goals(seed));
    let kit = travel(seed, 0, Habit::Commuter, 18, Variant::Kit, &w);
    let made = travel(seed, 0, Habit::Commuter, 18, Variant::Maker, &w);
    let bumps = |r: &Run| r.days.iter().map(|d| d.bumps).sum::<u32>();
    println!("bumps into the blocked way: kit {}, made {}", bumps(&kit), bumps(&made));
    assert!(bumps(&made) <= 5, "a made tool drops a blocked step at once");
    let roam = travel(seed, 2, Habit::Roamer, 18, Variant::Maker, &w);
    let fell: Vec<u32> = roam.days.iter().map(|d| d.fell_back).collect();
    println!("roamer fell back to the cheap version, by day: {fell:?}");
    assert!(fell.iter().sum::<u32>() > 0, "frightened by the dog, it uses the cheap version");
    assert!(fell.iter().enumerate().all(|(d, &f)| f == 0 || (DOG as usize..DOG as usize + 3).contains(&d)), "only when frightened");
}
