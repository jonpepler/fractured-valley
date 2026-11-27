//! Day-by-day measurements for the home experiment, shared by the tests, examples and viewers.
//! Everything here only reads the mind; nothing in the fox is special-cased for homes.

use crate::mind::Mind;
use crate::scenarios::{self, HomeMove, DAY, NIGHT};
use crate::sim::Sim;
use crate::spread::{Live, Mods};
use crate::world::{d2, Action, Kind, BURROW};

/// What one day of the home experiment looked like from inside the fox.
#[derive(Clone, Debug, Default)]
pub struct DayStats {
    pub day: u32,
    /// The place the fox slept at the night before (its believed home), and where the burrow was.
    pub slept_at: String,
    /// The largest absence error (an expected burrow missing), and the first tick of the day either
    /// it or a novel burrow surprised the fox (the measurements below count from there).
    pub absence: i32,
    pub absence_at: Option<u32>,
    /// The largest surprise at a burrow seen where none was expected.
    pub novel: i32,
    /// Peaks over the day: arousal, stress, loss on the mind; and the lowest mood.
    pub arousal: i32,
    pub stress: i32,
    pub loss: i32,
    pub mood_min: i32,
    /// Ticks of the day with arousal ≥ 500 and with stress ≥ 500; mean arousal over the 30 ticks
    /// after the first surprise; the learnt volatility of burrows at nightfall.
    pub aroused: u32,
    pub stressed: u32,
    pub arousal30: i32,
    pub volatility: i32,
    /// Ticks of the day spent searching, fleeing, napping, and near the old den (within 2 cells).
    pub searching: u32,
    pub fleeing: u32,
    pub napping: u32,
    pub near_old: u32,
    /// Ticks from the first absence to first seeing a burrow (anywhere), and to reaching one.
    pub to_see: Option<u32>,
    pub to_reach: Option<u32>,
    /// Confusion: goal switches and mean workspace turnover (‰ of the workspace replaced per tick)
    /// over the 30 ticks after the first absence.
    pub switches: u32,
    pub turnover: u32,
    /// The most frequent workspace nodes over those 30 ticks.
    pub thoughts: Vec<(String, u32)>,
    /// How the old den and the best learnt new place feel to a tired, calm fox at nightfall
    /// (before sleep), and which the fox then believes is home (where it sleeps).
    pub felt_old: i32,
    pub felt_new: i32,
    pub home_at_night: String,
}

/// How a place feels to a tired, calm fox (the home test).
pub fn felt_tired(m: &Mind, p: u32) -> i32 {
    m.felt_with(&Live { g: &m.g, fast: &m.fast }, p, &Mods { fatigue: 1000, ..Mods::default() })
}

/// Run `days` days of a variant; return one `DayStats` per day.
pub fn home_days(seed: u64, v: HomeMove, days: u32) -> (Sim, Vec<DayStats>) {
    let mut s = scenarios::home_moved(seed, v, days);
    let old = s.m.place_node(&s.w, "den");
    let (ox, oy) = (8, 30);
    let mut out = vec![];
    let mut slept_at = String::from("den");
    for d in 0..days {
        let mut st = DayStats { day: d + 1, mood_min: 1000, ..Default::default() };
        st.slept_at = slept_at.clone();
        let mut prev_ws: Vec<u32> = vec![];
        let mut prev_goal = None;
        let mut counts: Vec<(u32, u32)> = vec![];
        let (mut turn, mut nturn) = (0u32, 0u32);
        for k in 0..DAY {
            let t = s.w.tick;
            if k == NIGHT {
                // At nightfall, before sleep: how the places feel, and where the fox goes to sleep.
                let m = &s.m;
                st.felt_old = felt_tired(m, old);
                st.felt_new = (0..m.place_nodes.len()).filter(|&pi| m.place_learnt(pi) && m.g.names[m.place_nodes[pi] as usize].starts_with("new place"))
                    .map(|pi| felt_tired(m, m.place_nodes[pi])).max().unwrap_or(0);
                st.home_at_night = m.g.names[m.home() as usize].clone();
                st.volatility = m.volatility[Kind::Den.idx()] as i32;
                slept_at = st.home_at_night.clone();
                s.step();
                continue;
            }
            let a = s.step();
            let m = &s.m;
            let r = &m.last;
            for &(_, _, e) in &r.absent { if e > st.absence { st.absence = e; } if st.absence_at.is_none() { st.absence_at = Some(t); } }
            for p in &r.percepts { if p.kind == Kind::Den && p.error >= 400 {
                st.novel = st.novel.max(p.error);
                if st.absence_at.is_none() { st.absence_at = Some(t); }
            } }
            st.arousal = st.arousal.max(m.mods.arousal);
            st.stress = st.stress.max(m.mods.stress);
            st.loss = st.loss.max(m.act(m.ids.loss));
            st.mood_min = st.mood_min.min(m.mods.mood);
            if m.mods.arousal >= 500 { st.aroused += 1; }
            if m.mods.stress >= 500 { st.stressed += 1; }
            if r.searching { st.searching += 1; }
            if matches!(a, Action::Flee { .. }) { st.fleeing += 1; }
            if r.why.starts_with("napping") { st.napping += 1; }
            if d2(s.w.fox.x, s.w.fox.y, ox, oy) <= 4 && st.absence_at.is_some() { st.near_old += 1; }
            if let Some(t0) = st.absence_at {
                // A burrow at a place that isn't the old den: seen, and reached.
                let burrows: Vec<(i32, i32)> = s.w.entities.iter().filter(|e| e.alive && e.kind == Kind::Den && (e.id != BURROW || d2(e.x, e.y, ox, oy + 1) > 4)).map(|e| (e.x, e.y)).collect();
                if st.to_see.is_none() && r.percepts.iter().any(|p| p.kind == Kind::Den && d2(p.x, p.y, ox, oy + 1) > 4) { st.to_see = Some(t - t0); }
                if st.to_reach.is_none() && burrows.iter().any(|&(x, y)| (x - s.w.fox.x).abs().max((y - s.w.fox.y).abs()) <= 1) { st.to_reach = Some(t - t0); }
                if t - t0 < 30 {
                    st.arousal30 += m.mods.arousal / 30;
                    for &n in &r.ws { match counts.iter_mut().find(|c| c.0 == n) { Some(c) => c.1 += 1, None => counts.push((n, 1)) } }
                    if !prev_ws.is_empty() { turn += (r.ws.iter().filter(|n| !prev_ws.contains(n)).count() * 1000 / r.ws.len().max(1)) as u32; nturn += 1; }
                    if m.goal != prev_goal && m.goal.is_some() && prev_goal.is_some() { st.switches += 1; }
                }
            }
            prev_ws = r.ws.clone();
            prev_goal = s.m.goal;
        }
        counts.sort_by_key(|c| (std::cmp::Reverse(c.1), c.0));
        st.thoughts = counts.iter().take(5).map(|&(n, c)| (s.m.g.names[n as usize].clone(), c)).collect();
        st.turnover = if nturn > 0 { turn / nturn } else { 0 };
        out.push(st);
    }
    (s, out)
}

pub fn print_days(label: &str, days: &[DayStats]) {
    println!("{label}:");
    println!("  day slept-at     absence  novel peak-ar/st  ar30 aroused stressed loss  mood search flee nap nearA  see reach switch turn vol  felt old/new  home at night | thinking about");
    for d in days {
        let o = |x: Option<u32>| x.map_or("-".to_string(), |v| v.to_string());
        println!("  {:3} {:12} {:5}@{:<4} {:5} {:4}/{:<4} {:5} {:7} {:8} {:4} {:5} {:6} {:4} {:3} {:5} {:4} {:5} {:6} {:4} {:4} {:5}/{:<6} {:12} | {}",
            d.day, d.slept_at, d.absence, o(d.absence_at.map(|t| t % DAY)), d.novel, d.arousal, d.stress, d.arousal30, d.aroused, d.stressed, d.loss, d.mood_min, d.searching, d.fleeing, d.napping, d.near_old,
            o(d.to_see), o(d.to_reach), d.switches, d.turnover, d.volatility, d.felt_old, d.felt_new, d.home_at_night,
            d.thoughts.iter().map(|(n, c)| format!("{n}×{c}")).collect::<Vec<_>>().join(", "));
    }
}
