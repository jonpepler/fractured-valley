//! Runs the 20-day scenario, prints the day-by-day table and a few explained
//! decisions, and writes `results/appetite.csv` and `results/appetite.svg`.

use fv_brains_appetite::brain::{Action, Decision, Habit, Why};
use fv_brains_appetite::body::TICKS_PER_DAY;
use fv_brains_appetite::sim::{run, Config, Run};

fn main() {
    let r = run(Config::default());
    println!("day  work_h  activity_kJ  intake_kJ  out_kJ  stored_kJ  weigh-in_kg  fat_kg  appetite");
    for d in &r.days {
        println!("{:>3}  {:>6.1}  {:>11.0}  {:>9.0}  {:>6.0}  {:>+9.0}  {:>11.2}  {:>6.2}  {:>8.2}", d.day, d.work_minutes as f64 / 60.0,
            d.activity.kj(), d.intake.kj(), d.out.kj(), d.stored_change.kj(), d.mass.kg(), d.fat.kg(), d.appetite.f());
    }
    println!("\nenergy gap, worst over every tick: {} mJ    final state hash: {:016x}", r.worst_gap.0, r.hash);

    println!("\nnights (retrospection): demand = spent / quiet day, hungry = share of waking intervals hungry");
    for (i, n) in r.nights.iter().enumerate() {
        println!("  after day {:>2}: demand {:.2}  hungry {:.2}  week's weight {:+.2} kg  target {:.2}  appetite {:.2} -> {:.2}",
            i + 1, n.demand.f(), n.hungry_share.f(), n.mass_change_week.kg(), n.target.f(), n.before.f(), n.after.f());
    }

    println!("\nsome decisions, explained:");
    let at = |day: u64, h: u64, m: u64| (day - 1) * TICKS_PER_DAY + h * 60 + m;
    let pick = |f: &dyn Fn(&Decision) -> bool| r.decisions.iter().find(|d| f(d)).copied();
    let shown = [
        pick(&|d| d.tick >= at(8, 10, 0) && d.chosen == Action::Work),
        pick(&|d| d.tick >= at(8, 10, 0) && d.chosen == Action::Eat && matches!(d.why, Why::Utility { .. })),
        pick(&|d| d.tick >= at(9, 9, 0) && matches!(d.why, Why::Reflex(_))),
        pick(&|d| d.tick > at(9, 9, 0) && matches!(d.why, Why::Utility { .. })),
        pick(&|d| d.tick >= at(15, 15, 0) && d.chosen == Action::Eat && matches!(d.why, Why::Utility { .. })),
    ];
    for d in shown.iter().flatten() { println!("  {}", d.explain().replace('\n', "\n  ")) }

    println!("\nthe same scenario with the habit learned differently:");
    for h in [Habit::Learned, Habit::Fixed, Habit::Instant] {
        let c = run(Config { habit: h, ..Config::default() });
        let mean = |a, b, f: fn(&fv_brains_appetite::sim::Day) -> i64| c.mean(a, b, f) as f64 / 1e6;
        println!("  {:<8} fat day 8 -> 20: {:+.2} kg   weigh-in mean d6-8 -> d18-20: {:+.2} kg   intake/out d10-14: {:.0}/{:.0} kJ",
            format!("{h:?}"), c.days[19].fat.kg() - c.days[7].fat.kg(),
            mean(18, 20, |d| d.mass.0) - mean(6, 8, |d| d.mass.0), mean(10, 14, |d| d.intake.0), mean(10, 14, |d| d.out.0));
    }

    std::fs::create_dir_all("results").unwrap();
    std::fs::write("results/appetite.csv", r.csv()).unwrap();
    std::fs::write("results/appetite.svg", chart(&r)).unwrap();
    println!("\nwrote results/appetite.csv and results/appetite.svg");
}

/// Four small panels over the same days, one y-scale each. Light theme only.
fn chart(r: &Run) -> String {
    const W: f64 = 720.0;
    const PH: f64 = 150.0;
    const L: f64 = 64.0;
    const RM: f64 = 92.0;
    let (blue, orange, ink, ink2, grid) = ("#257de0", "#f47230", "#191919", "#666666", "#e6e6e6");
    let n = r.days.len() as f64;
    let x = |day: f64| L + (day - 1.0) / (n - 1.0) * (W - L - RM);
    type Series<'a> = (&'a str, &'a str, Vec<f64>);
    let panels: Vec<(&str, Vec<Series>)> = vec![
        ("Energy per day (MJ)", vec![
            ("eaten", blue, r.days.iter().map(|d| d.intake.kj() / 1e3).collect()),
            ("out", orange, r.days.iter().map(|d| d.out.kj() / 1e3).collect()),
        ]),
        ("Activity above resting (MJ/day)", vec![("", blue, r.days.iter().map(|d| d.activity.kj() / 1e3).collect())]),
        ("Appetite habit (1 = neutral)", vec![("", blue, r.days.iter().map(|d| d.appetite.f()).collect())]),
        ("Body fat (kg)", vec![("", blue, r.days.iter().map(|d| d.fat.kg()).collect())]),
    ];
    let top = 48.0;
    let gap = 40.0;
    let h = top + panels.len() as f64 * (PH + gap) + 20.0;
    let mut s = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{W}\" height=\"{h}\" viewBox=\"0 0 {W} {h}\" font-family=\"Helvetica, Arial, sans-serif\">\n<rect width=\"100%\" height=\"100%\" fill=\"#ffffff\"/>\n");
    s += &format!("<text x=\"{L}\" y=\"24\" font-size=\"16\" fill=\"{ink}\" font-weight=\"bold\">One person, 20 days: injured on day 9, appetite fades slower than activity</text>\n");
    for (i, (title, series)) in panels.iter().enumerate() {
        let y0 = top + i as f64 * (PH + gap) + 18.0;
        let all: Vec<f64> = series.iter().flat_map(|x| x.2.iter().copied()).collect();
        let (mut lo, mut hi) = (all.iter().cloned().fold(f64::MAX, f64::min), all.iter().cloned().fold(f64::MIN, f64::max));
        if i == 1 || i == 0 { lo = 0.0 }
        let pad = (hi - lo) * 0.08;
        hi += pad;
        if i > 1 { lo -= pad }
        let y = |v: f64| y0 + PH - (v - lo) / (hi - lo) * PH;
        s += &format!("<text x=\"{L}\" y=\"{}\" font-size=\"13\" fill=\"{ink}\">{title}</text>\n", y0 - 6.0);
        for k in 0..=3 {
            let v = lo + (hi - lo) * k as f64 / 3.0;
            s += &format!("<line x1=\"{L}\" x2=\"{}\" y1=\"{yy:.1}\" y2=\"{yy:.1}\" stroke=\"{grid}\"/><text x=\"{}\" y=\"{:.1}\" font-size=\"11\" fill=\"{ink2}\" text-anchor=\"end\">{v:.1}</text>\n",
                W - RM, L - 6.0, y(v) + 4.0, yy = y(v));
        }
        let xi = x(9.0);
        s += &format!("<line x1=\"{xi:.1}\" x2=\"{xi:.1}\" y1=\"{y0}\" y2=\"{}\" stroke=\"{ink2}\" stroke-dasharray=\"4 3\"/>\n", y0 + PH);
        if i == 0 { s += &format!("<text x=\"{:.1}\" y=\"{}\" font-size=\"11\" fill=\"{ink2}\">injury, day 9</text>\n", xi + 4.0, y0 + 10.0) }
        for (name, colour, v) in series {
            let pts: Vec<String> = v.iter().enumerate().map(|(d, v)| format!("{:.1},{:.1}", x(d as f64 + 1.0), y(*v))).collect();
            s += &format!("<polyline points=\"{}\" fill=\"none\" stroke=\"{colour}\" stroke-width=\"2\" stroke-linejoin=\"round\"/>\n", pts.join(" "));
            if !name.is_empty() {
                let last = *v.last().unwrap();
                let dy = if *name == "eaten" { -10.0 } else { 10.0 };
                s += &format!("<circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"4\" fill=\"{colour}\" stroke=\"#fff\" stroke-width=\"2\"/><text x=\"{:.1}\" y=\"{:.1}\" font-size=\"12\" fill=\"{ink}\">{name}</text>\n",
                    x(n), y(last), x(n) + 8.0, y(last) + dy + 4.0);
            }
        }
        for d in [1, 5, 9, 13, 17, 20] {
            s += &format!("<text x=\"{:.1}\" y=\"{}\" font-size=\"11\" fill=\"{ink2}\" text-anchor=\"middle\">{}</text>\n", x(d as f64), y0 + PH + 14.0,
                d);
        }
    }
    s += &format!("<text x=\"{:.1}\" y=\"{}\" font-size=\"11\" fill=\"{ink2}\">day</text>\n", x(n) + 14.0, h - 28.0);
    s += "</svg>\n";
    s
}
