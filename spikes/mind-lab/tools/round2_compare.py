#!/usr/bin/env python3
"""Round 2, stage 1: round 1's village (runs.csv from the old code) against round 2's, paired by
seed, sign-tested. python3 tools/round2_compare.py <round1 runs.csv> <round2 runs.csv>"""
import csv, math, sys
def load(p): return {int(r["seed"]): r for r in csv.DictReader(open(p))}
a, b = load(sys.argv[1]), load(sys.argv[2])
seeds = sorted(set(a) & set(b))
def sign(x, y):
    up = sum(1 for p, q in zip(x, y) if q > p); dn = sum(1 for p, q in zip(x, y) if q < p); n = up + dn
    if n == 0: return up, dn, 1.0
    z = (abs(up - dn) - 1) / math.sqrt(n)
    return up, dn, math.erfc(max(z, 0) / math.sqrt(2))
keys = ["meals", "drinks", "gossip", "help", "proposals", "caught", "gored", "surprised", "said", "heard_per_tick", "trust1", "warmth1", "warm_sat", "total_b", "places", "danger_ticks", "surprised_per_1000_thinks"]
print(f"| Measure (per valley, 30 days) | Round 1 | Round 2 | Change | Round 2 higher / lower | p |")
print("|---|---|---|---|---|---|")
for k in keys:
    x = [float(a[s][k]) for s in seeds]; y = [float(b[s][k]) for s in seeds]
    ma, mb = sum(x) / len(x), sum(y) / len(y)
    up, dn, p = sign(x, y)
    ch = f"{(mb - ma) / ma * 100:+.0f}%" if ma else "–"
    ps = "< 0.001" if p < 0.001 else f"{p:.3f}"
    print(f"| {k} | {ma:.4g} | {mb:.4g} | {ch} | {up} / {dn} | {ps} |")
print(f"\n{len(seeds)} paired seeds")
