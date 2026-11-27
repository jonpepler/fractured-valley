#!/usr/bin/env python3
"""Round 2, stage 3: the danger, teaching and hunt scenarios at each thinking rate against every
tick, paired by case, sign-tested, with d_z. python3 tools/rate_scenarios.py <tag> [--md]"""
import csv, math, sys
tag = sys.argv[1]; md = '--md' in sys.argv
def load(n): return list(csv.DictReader(open(f"out/rate/{n}{tag}.csv")))
def sign(x, y):
    up = sum(1 for p, q in zip(x, y) if q > p); dn = sum(1 for p, q in zip(x, y) if q < p); n = up + dn
    if n == 0: return up, dn, 1.0
    z = (abs(up - dn) - 1) / math.sqrt(n); return up, dn, min(1.0, math.erfc(max(z, 0) / math.sqrt(2)))
def dz(x, y):
    d = [q - p for p, q in zip(x, y)]; n = len(d); m = sum(d) / n
    sd = math.sqrt(sum((v - m) ** 2 for v in d) / (n - 1)) if n > 1 else 0
    return m / sd if sd > 0 else 0.0
out = []
def table(name, rows, key, measures, derived=None):
    by = {}
    for r in rows: by.setdefault(int(r['every']), {})[tuple(r[k] for k in key)] = r
    rates = sorted(by)
    for m in measures:
        f = derived[m] if derived and m in derived else (lambda r, m=m: float(r[m]))
        base = by[1]
        for e in rates[1:]:
            ks = sorted(set(base) & set(by[e]))
            x = [f(base[k]) for k in ks]; y = [f(by[e][k]) for k in ks]
            up, dn, p = sign(x, y)
            out.append((name, m, e, len(ks), sum(x) / len(x), sum(y) / len(y), up, dn, p, dz(x, y)))
d = load("danger"); t = load("teaching"); h = load("hunt")
table("danger", d, ("seed", "temper", "allies", "expected"), ["error", "arousal", "stress_peak", "flee", "fight", "hold", "gored", "boar_fled"],
      {"flee": lambda r: float(r["flee"]), "fight": lambda r: float(r["fight"])})
table("danger, surprised", [r for r in d if r["expected"] == "0"], ("seed", "temper", "allies"), ["error", "arousal", "flee", "gored"])
table("teaching", t, ("seed", "set"), ["told_spring", "told_first_tick", "untold_spring", "told_sooner", "cai_drank_spring"])
table("hunt", h, ("seed",), ["planned_caught", "solo_caught", "planned_effort", "solo_effort", "joined"])
# Holm over every comparison at each rate.
rates = sorted(set(o[2] for o in out))
adj = {}
for e in rates:
    idx = [i for i, o in enumerate(out) if o[2] == e]
    order = sorted(idx, key=lambda i: out[i][8]); run = 0.0
    for k, i in enumerate(order): run = max(run, min(1.0, (len(idx) - k) * out[i][8])); adj[i] = run
if md: print("| Scenario | Measure | Every | Cases | Every tick | This rate | Higher / lower | p | p (Holm) | d_z |\n|---|---|---|---|---|---|---|---|---|---|")
for i, (n, m, e, k, a, b, up, dn, p, d_) in enumerate(out):
    ps = "< 0.001" if p < 0.001 else f"{p:.3f}"; pa = adj[i]; pas = "< 0.001" if pa < 0.001 else f"{pa:.3f}"
    if md: print(f"| {n} | {m} | {e} | {k} | {a:.4g} | {b:.4g} | {up} / {dn} | {ps} | {pas} | {d_:+.2f} |")
    else: print(f"{n:18s} {m:16s} e{e:<3d} n{k:4d} {a:9.4g} {b:9.4g} {up:4d}/{dn:<4d} p {ps:>7s} holm {pas:>7s} dz {d_:+.2f}")
if '--csv' in sys.argv:
    with open(sys.argv[sys.argv.index('--csv') + 1], 'w') as f:
        w = csv.writer(f); w.writerow(["scenario", "measure", "every", "cases", "every_tick_mean", "rate_mean", "higher", "lower", "p", "p_holm", "d_z"])
        for i, o in enumerate(out): w.writerow(list(o[:8]) + [f"{o[8]:.6g}", f"{adj[i]:.6g}", f"{o[9]:.4f}"])
