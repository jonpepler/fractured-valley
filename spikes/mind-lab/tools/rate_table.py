#!/usr/bin/env python3
"""Round 2, stage 3: one markdown table from a rate_compare.py --csv file: each measure's every-tick
mean, then per mode its change, d_z, and whether it differs (Holm-corrected sign test, 0.05: **bold**).
python3 tools/rate_table.py <compare.csv> <mode,mode,...> [label;label;...]"""
import csv, sys
rows = list(csv.DictReader(open(sys.argv[1]))); modes = sys.argv[2].split(',')
labels = sys.argv[3].split(';') if len(sys.argv) > 3 else modes
NAMES = {"meals": "Meals", "drinks": "Drinks", "gossip": "Gossip", "help": "Cries for help", "proposals": "Hunts proposed", "caught": "Deer caught", "gored": "Gorings",
 "surprised": "Mind-ticks surprised", "said": "Utterances", "heard_per_tick": "Words heard a person-tick", "trust1": "Mean trust at the end", "warmth1": "Mean warmth at the end",
 "warm_sat": "Feelings at the ceiling", "total_b": "Bytes a mind on day 30", "places": "Places known", "danger_ticks": "Ticks in a danger response", "surprised_per_1000_thinks": "Surprised per 1,000 thinks",
 "told": "Told records a mind", "thinks": "Thinks", "woken": "Thinks off turn (woken)", "brain_us_per_mind_tick": "µs per mind-tick (parallel runs, rough)"}
by = {(r["mode"], r["measure"]): r for r in rows}
keys = [k for k in NAMES if (modes[0], k) in by]
print("| Measure (per valley, 30 days) | Every tick | " + " | ".join(labels) + " |")
print("|---|---|" + "---|" * len(modes))
for k in keys:
    base = float(by[(modes[0], k)]["base_mean"])
    cells = []
    for m in modes:
        r = by.get((m, k))
        if not r: cells.append("–"); continue
        b, v = float(r["base_mean"]), float(r["mode_mean"]); d = float(r["d_z"])
        ch = f"{(v - b) / b * 100:+.0f}%" if b else f"{v:.3g}"
        s = f"{ch} ({d:+.1f})"
        if r["p_holm"] and float(r["p_holm"]) < 0.05: s = f"**{s}**"
        cells.append(s)
    fb = f"{base:,.0f}" if abs(base) >= 100 else f"{base:.3g}"
    print(f"| {NAMES[k]} | {fb} | " + " | ".join(cells) + " |")
