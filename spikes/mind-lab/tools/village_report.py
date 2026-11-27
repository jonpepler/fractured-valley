#!/usr/bin/env python3
"""Stage 5: tables and charts from `out/village/*.csv` (written by `--example village_study`).
python3 tools/village_report.py [out/village] [chart dir]"""
import csv, math, os, sys
from collections import defaultdict

D = sys.argv[1] if len(sys.argv) > 1 else "out/village"
IMG = sys.argv[2] if len(sys.argv) > 2 else "out/village/img"
os.makedirs(IMG, exist_ok=True)

def load(name):
    p = os.path.join(D, name)
    if not os.path.exists(p): return []
    with open(p) as f: return list(csv.DictReader(f))

def num(x):
    try: return float(x)
    except ValueError: return x

runs = [{k: num(v) for k, v in r.items()} for r in load("runs.csv")]
days = [{k: num(v) for k, v in r.items()} for r in load("days.csv")]
people = [{k: num(v) for k, v in r.items()} for r in load("people.csv")]
by = defaultdict(dict)
for r in runs: by[r["mode"]][int(r["seed"])] = r

def mean(xs): xs = list(xs); return sum(xs) / len(xs) if xs else float("nan")
def sd(xs): xs = list(xs); m = mean(xs); return math.sqrt(sum((x - m) ** 2 for x in xs) / max(1, len(xs) - 1))

def sign_test(a, b):
    """Two-sided sign test p for paired lists (ties dropped), by the normal approximation."""
    up = sum(1 for x, y in zip(a, b) if y > x); dn = sum(1 for x, y in zip(a, b) if y < x); n = up + dn
    if n == 0: return up, dn, 1.0
    z = (abs(up - dn) - 1) / math.sqrt(n)
    return up, dn, math.erfc(max(z, 0) / math.sqrt(2))

def compare(m0, m1, keys):
    seeds = sorted(set(by[m0]) & set(by[m1]))
    print(f"\n### {m0} against {m1} ({len(seeds)} paired seeds)\n")
    print(f"| measure (per run) | {m0} mean | {m1} mean | change | {m1} higher / lower | sign test p |")
    print("|---|---|---|---|---|---|")
    for k in keys:
        a = [by[m0][s][k] for s in seeds]; b = [by[m1][s][k] for s in seeds]
        up, dn, p = sign_test(a, b)
        ch = (mean(b) - mean(a)) / mean(a) * 100 if mean(a) else float("nan")
        print(f"| {k} | {mean(a):.1f} | {mean(b):.1f} | {ch:+.1f}% | {up} / {dn} | {p:.3g} |")

KEYS = ["meals", "drinks", "gossip", "help", "proposals", "caught", "gored", "surprised", "said", "danger_ticks", "feast", "trust1", "warmth1", "total_b", "own_b", "places", "brain_us_per_mind_tick"]
if "full" in by and "cheap" in by: compare("full", "cheap", KEYS)
if "full" in by and "prune" in by: compare("full", "prune", KEYS + ["pruned_places", "pruned_links"])
if "full" in by and "oldvoice" in by: compare("full", "oldvoice", ["heard_per_tick", "air_per_tick", "gossip", "said", "drinks", "meals", "surprised", "trust1", "warmth1"])

for m in by:
    rs = list(by[m].values())
    print(f"\n{m}: {len(rs)} runs; µs per mind-tick {mean(r['brain_us_per_mind_tick'] for r in rs):.1f} (sd {sd(r['brain_us_per_mind_tick'] for r in rs):.1f}); "
          f"thinks {mean(r['thinks'] for r in rs):.0f}, skipped {mean(r['skipped'] for r in rs):.0f}, woken {mean(r['woken'] for r in rs):.0f}; "
          f"trust {mean(r['trust0'] for r in rs):.0f} → {mean(r['trust1'] for r in rs):.0f}; warmth {mean(r['warmth0'] for r in rs):.0f} → {mean(r['warmth1'] for r in rs):.0f}; "
          f"saturated warm {mean(r['warm_sat'] for r in rs)*100:.2f}% cold {mean(r['cold_sat'] for r in rs)*100:.2f}%; heard per mind-tick {mean(r['heard_per_tick'] for r in rs):.3f}; utterances in the air per tick {mean(r['air_per_tick'] for r in rs):.3f}")
    trust_up = sum(1 for r in rs if r["trust1"] > r["trust0"]); warm_up = sum(1 for r in rs if r["warmth1"] > r["warmth0"])
    print(f"  trust rose in {trust_up}/{len(rs)} valleys, warmth in {warm_up}/{len(rs)}")

# Storage over time (mean over seeds of the mean over minds).
COMP = ["base_share", "places_b", "people_b", "sources_b", "plans_b", "notes_b", "other_own_b", "state", "layer"]
for m in sorted(set(r["mode"] for r in days)):
    ds = defaultdict(list)
    for r in days:
        if r["mode"] == m: ds[int(r["day"])].append(r)
    print(f"\n### Storage per mind, {m} (bytes, mean over {len(ds[0])} valleys)\n")
    print("| night | before sleep | after sleep | fast before | places | people | sources | plans | other own | state (of it expectations) | layer | places known | source links | nodes | links |")
    print("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|")
    for d in sorted(ds):
        if d in (0, 1, 2, 4, 9, 14, 19, 24, 29) or d == max(ds):
            g = ds[d]; f = lambda k: mean(x[k] for x in g)
            print(f"| {d+1} | {f('before_total'):.0f} | {f('after_total'):.0f} | {f('fast_before'):.0f} | {f('places_b'):.0f} | {f('people_b'):.0f} | {f('sources_b'):.0f} | {f('plans_b'):.0f} | {f('other_own_b'):.0f} | {f('state'):.0f} ({f('expect'):.0f}) | {f('layer'):.0f} | {f('place_count'):.1f} | {f('source_links'):.1f} | {f('nodes'):.1f} | {f('links'):.0f} |")
    try:
        import matplotlib; matplotlib.use("Agg"); import matplotlib.pyplot as plt
        xs = sorted(ds)
        fig, ax = plt.subplots(figsize=(9, 5))
        cols = ["#2a6f97", "#61a5c2", "#a9d6e5", "#e9c46a", "#f4a261", "#e76f51", "#8d99ae", "#6a994e", "#bc4749"]
        stack = [[mean(x[k] for x in ds[d]) for d in xs] for k in COMP]
        ax.stackplot([d + 1 for d in xs], stack, labels=["base (share)", "places", "people", "source links", "plans", "notes", "other own", "state", "people layer"], colors=cols, alpha=0.9)
        ax.plot([d + 1 for d in xs], [mean(x["before_total"] for x in ds[d]) for d in xs], color="black", lw=1.5, label="total before sleep")
        ax.set_xlabel("night"); ax.set_ylabel("bytes per mind (mean over minds and valleys)")
        ax.set_title(f"Storage per mind over a month ({m}, {len(ds[0])} valleys, 30 people)")
        ax.legend(loc="upper left", fontsize=8, ncol=2); ax.grid(alpha=0.3)
        fig.tight_layout(); fig.savefig(os.path.join(IMG, f"storage-{m}.png"), dpi=130); plt.close(fig)
    except ImportError:
        pass

# Per-day activity.
for m in ["full"]:
    ds = defaultdict(list)
    for r in days:
        if r["mode"] == m: ds[int(r["day"])].append(r)
    if not ds: continue
    print(f"\n### Activity per day ({m}, mean over valleys, whole village)\n")
    print("| day | meals | drinks | gossip | help calls | plans proposed | deer caught | surprised mind-ticks |")
    print("|---|---|---|---|---|---|---|---|")
    for d in sorted(ds):
        if d in (0, 1, 2, 4, 9, 14, 19, 24, 29):
            g = ds[d]; f = lambda k: mean(x[k] for x in g)
            print(f"| {d+1} | {f('meals'):.1f} | {f('drinks'):.1f} | {f('gossip'):.1f} | {f('help'):.2f} | {f('proposals'):.2f} | {f('caught'):.2f} | {f('surprised'):.0f} |")

# Character.
def ranks(xs):
    o = sorted(range(len(xs)), key=lambda i: xs[i]); r = [0.0] * len(xs); i = 0
    while i < len(o):
        j = i
        while j + 1 < len(o) and xs[o[j + 1]] == xs[o[i]]: j += 1
        for k in range(i, j + 1): r[o[k]] = (i + j) / 2
        i = j + 1
    return r
def spearman(a, b):
    ra, rb = ranks(a), ranks(b); ma, mb = mean(ra), mean(rb)
    num_ = sum((x - ma) * (y - mb) for x, y in zip(ra, rb)); den = math.sqrt(sum((x - ma) ** 2 for x in ra) * sum((y - mb) ** 2 for y in rb))
    return num_ / den if den else float("nan")
ps = [p for p in people if p["mode"] == "full"]
if ps:
    print(f"\n### Character ({len(ps)} villagers, full detail): Spearman correlations\n")
    ps2 = [dict(p, home=p["at0"] / sum(p[f"at{k}"] for k in range(8)), spring=p["at2"] / sum(p[f"at{k}"] for k in range(8)), talk=sum(p[f"said{k}"] for k in range(8))) for p in ps]
    pairs = [("reactivity", "surprised"), ("reactivity", "danger"), ("aggression", "danger"), ("reactivity", "walked"), ("honesty", "warmth_in"), ("helpful", "said1"),
             ("waters", "drinks"), ("bushes", "meals"), ("places", "walked"), ("warmth_out", "gossip_said"), ("warmth_out", "best_close"), ("trust_out", "told"),
             ("gossip_heard", "warmth_out"), ("reactivity", "home"), ("waters", "spring")]
    print("| a | b | ρ |"); print("|---|---|---|")
    for a, b in pairs: print(f"| {a} | {b} | {spearman([p[a] for p in ps2], [p[b] for p in ps2]):+.2f} |")
    # Spread of lives: how different are people in the same valley?
    def cv(k): return mean(sd([p[k] for p in ps2 if p["seed"] == s]) / max(1e-9, mean(p[k] for p in ps2 if p["seed"] == s)) for s in set(p["seed"] for p in ps2))
    print("\nWithin-valley spread (coefficient of variation, mean over valleys): " + ", ".join(f"{k} {cv(k):.2f}" for k in ["walked", "drinks", "meals", "surprised", "danger", "talk", "gossip_said", "best_close", "home"]))
    print("\nSeed 1, a few villagers:\n")
    print("| who | reactivity | honesty | home % | stream % | spring % | bushes % | walked | drinks | meals | best friend (ticks close) | warmth out / in | knows places / water / bushes | told | surprised | said | gossip said |")
    print("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|")
    s1 = [p for p in ps2 if p["seed"] == 1]
    s1.sort(key=lambda p: p["reactivity"])
    for p in s1[:3] + s1[len(s1)//2 - 1:len(s1)//2 + 1] + s1[-3:]:
        tot = sum(p[f"at{k}"] for k in range(8))
        print(f"| {int(p['i'])} | {p['reactivity']:.0f} | {p['honesty']:.0f} | {p['at0']*100/tot:.0f} | {p['at1']*100/tot:.0f} | {p['at2']*100/tot:.0f} | {p['at3']*100/tot:.0f} | {p['walked']:.0f} | {p['drinks']:.0f} | {p['meals']:.0f} | {int(p['best_friend'])} ({p['best_close']:.0f}) | {p['warmth_out']:.0f} / {p['warmth_in']:.0f} | {p['places']:.0f} / {p['waters']:.0f} / {p['bushes']:.0f} | {p['told']:.0f} | {p['surprised']:.0f} | {p['talk']:.0f} | {p['gossip_said']:.0f} |")
