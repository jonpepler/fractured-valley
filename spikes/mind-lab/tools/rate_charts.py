#!/usr/bin/env python3
"""Round 2, stage 3 charts. python3 tools/rate_charts.py <compare.csv> <scenarios.csv> <bench.csv> <outdir>
compare.csv: tools/rate_compare.py --csv; scenarios.csv: tools/rate_scenarios.py --csv;
bench.csv: rate_bench lines (target,mode,n,days,us,thinks,hash)."""
import csv, sys, os, math
import matplotlib; matplotlib.use("Agg")
import matplotlib.pyplot as plt
cmp_, scen, bench, outd = sys.argv[1:5]
os.makedirs(outd, exist_ok=True)
BLUE, ORANGE, AQUA, YELLOW, VIOLET = "#257de0", "#f47230", "#00b894", "#f2a600", "#6a4ce2"
TEXT, TEXT2, GRID = "#191919", "#666666", "#e6e6e6"
plt.rcParams.update({"font.size": 10, "axes.edgecolor": TEXT2, "axes.labelcolor": TEXT, "xtick.color": TEXT2, "ytick.color": TEXT2,
                     "axes.spines.top": False, "axes.spines.right": False, "axes.grid": True, "grid.color": GRID, "grid.linewidth": 0.8, "lines.linewidth": 2})
C = list(csv.DictReader(open(cmp_)))
S = list(csv.DictReader(open(scen)))
B = [r for r in csv.reader(open(bench)) if r and not r[0].startswith('#')]
KEYS = ["meals", "drinks", "gossip", "help", "proposals", "caught", "gored", "surprised", "said", "heard_per_tick", "trust1", "warmth1", "warm_sat", "total_b", "places", "danger_ticks", "surprised_per_1000_thinks"]
def village(mode):
    rows = [r for r in C if r["mode"] == mode and r["measure"] in KEYS]
    sig = sum(1 for r in rows if r["p_holm"] and float(r["p_holm"]) < 0.05)
    d = sorted(abs(float(r["d_z"])) for r in rows)
    return sig, sum(d) / len(d) if d else 0
rates = [1, 2, 4, 10, 20]
# 1. Rate against faithfulness: village measures differing, and the scenarios' measures differing.
fig, ax = plt.subplots(figsize=(7, 4))
vs = [0] + [village(f"e{e}")[0] for e in rates[1:]]
def scen_sig(e, name=None):
    rows = [r for r in S if int(r["every"]) == e and (name is None or r["scenario"].startswith(name))]
    return sum(1 for r in rows if float(r["p_holm"]) < 0.05), len(rows)
ss = [0] + [scen_sig(e)[0] for e in rates[1:]]
ax.plot(rates, vs, color=BLUE, marker="o", markersize=8, label=f"village: of {len(KEYS)} measures")
ax.plot(rates, ss, color=ORANGE, marker="s", markersize=8, label=f"danger, teaching, hunts: of {scen_sig(2)[1]} measures")
ax.set_xscale("log"); ax.set_xticks(rates); ax.set_xticklabels([str(r) for r in rates])
ax.set_xlabel("every mind thinks every N ticks (20 = once a second)"); ax.set_ylabel("measures that differ from every tick\n(Holm-corrected sign test, 0.05)")
for x, y in zip(rates, vs): ax.annotate(str(y), (x, y), textcoords="offset points", xytext=(0, 8), ha="center", color=TEXT, fontsize=9)
for x, y in zip(rates, ss): ax.annotate(str(y), (x, y), textcoords="offset points", xytext=(0, -15), ha="center", color=TEXT, fontsize=9)
ax.legend(frameon=False, loc="upper left"); ax.set_ylim(bottom=-1)
ax.set_title("Thinking less often: how many outcomes move", color=TEXT, loc="left")
fig.tight_layout(); fig.savefig(f"{outd}/rate-faithfulness.png", dpi=150); plt.close(fig)
# 2. Change in key village measures by rate.
fig, ax = plt.subplots(figsize=(7, 4))
for k, col, lab in [("meals", BLUE, "meals"), ("drinks", AQUA, "drinks"), ("gossip", ORANGE, "gossip"), ("warmth1", VIOLET, "warmth at the end"), ("total_b", YELLOW, "bytes a mind")]:
    ys = [0]
    for e in rates[1:]:
        r = next(r for r in C if r["mode"] == f"e{e}" and r["measure"] == k)
        ys.append((float(r["mode_mean"]) - float(r["base_mean"])) / float(r["base_mean"]) * 100)
    ax.plot(rates, ys, color=col, marker="o", markersize=7, label=lab)
    ax.annotate(lab, (rates[-1], ys[-1]), textcoords="offset points", xytext=(6, 0), va="center", color=TEXT, fontsize=8)
ax.axhline(0, color=TEXT2, linewidth=1)
ax.set_xscale("log"); ax.set_xticks(rates); ax.set_xticklabels([str(r) for r in rates]); ax.set_xlim(0.9, 40)
ax.set_xlabel("every mind thinks every N ticks"); ax.set_ylabel("change against every tick (%)")
ax.legend(frameon=False, ncol=3, loc="upper left", fontsize=8)
ax.set_title("Village measures over 200 paired valleys × 30 days", color=TEXT, loc="left")
fig.tight_layout(); fig.savefig(f"{outd}/rate-village-measures.png", dpi=150); plt.close(fig)
# 3. Hunts: deer caught by rate.
fig, ax = plt.subplots(figsize=(7, 3.6))
for m, col, lab in [("planned_caught", BLUE, "planned hunt"), ("solo_caught", ORANGE, "hunting alone")]:
    ys = []
    for e in rates:
        if e == 1: r = next(r for r in S if r["measure"] == m and int(r["every"]) == 2); ys.append(float(r["every_tick_mean"]) * 100)
        else: r = next(r for r in S if r["measure"] == m and int(r["every"]) == e); ys.append(float(r["rate_mean"]) * 100)
    ax.plot(rates, ys, color=col, marker="o", markersize=7, label=lab)
ax.set_xscale("log"); ax.set_xticks(rates); ax.set_xticklabels([str(r) for r in rates]); ax.set_ylim(0, 100)
ax.set_xlabel("every mind thinks every N ticks"); ax.set_ylabel("hunts that catch the deer (%)")
ax.legend(frameon=False); ax.set_title("Hunts over 200 valleys", color=TEXT, loc="left")
fig.tight_layout(); fig.savefig(f"{outd}/rate-hunts.png", dpi=150); plt.close(fig)
# 4. Faithfulness per microsecond: mean |d_z| over the 17 village measures against cost.
cost = {r[1]: float(r[4]) for r in B if r[0] == "native"}
fig, ax = plt.subplots(figsize=(7, 4.2))
groups = [("slower full thinking", BLUE, ["e1", "e2", "e4", "e10", "e20"]), ("shallower, every tick", ORANGE, ["e1", "s317", "s634", "s950"]), ("small brain between big thinks", AQUA, ["e1", "b4", "b20"])]
for lab, col, ms in groups:
    pts = [(cost[m], 0 if m == "e1" else village(m)[1], m) for m in ms if m in cost and (m == "e1" or any(r["mode"] == m for r in C))]
    ax.plot([p[0] for p in pts], [p[1] for p in pts], color=col, marker="o", markersize=8, label=lab)
    for x, y, m in pts:
        if m != "e1" or lab.startswith("slower"): ax.annotate(m, (x, y), textcoords="offset points", xytext=(6, 4), color=TEXT, fontsize=8)
ax.set_xlabel("µs per mind-tick (native, one thread)"); ax.set_ylabel("mean |d_z| over the 17 village measures\n(0 = every tick)")
ax.legend(frameon=False); ax.set_title("Faithfulness against cost", color=TEXT, loc="left")
fig.tight_layout(); fig.savefig(f"{outd}/cost-faithfulness.png", dpi=150); plt.close(fig)
print("charts written to", outd)
