#!/usr/bin/env python3
"""Round 2, stage 3: each mode of a runs.csv (village_study) against a baseline mode, paired by
seed: means, change, sign test (two-sided, normal approximation with continuity correction, as
round2_compare.py), and the paired effect size d_z (mean paired difference / its SD).
python3 tools/rate_compare.py <runs.csv> <baseline mode> <mode,mode,...> [--md] [--csv out.csv]"""
import csv, math, sys
rows = list(csv.DictReader(open(sys.argv[1])))
base = sys.argv[2]; modes = sys.argv[3].split(',')
md = '--md' in sys.argv
out = sys.argv[sys.argv.index('--csv') + 1] if '--csv' in sys.argv else None
by = {}
for r in rows: by.setdefault(r['mode'], {})[int(r['seed'])] = r
KEYS = ["meals", "drinks", "gossip", "help", "proposals", "caught", "gored", "surprised", "said", "heard_per_tick", "trust1", "warmth1", "warm_sat", "total_b", "places", "danger_ticks", "surprised_per_1000_thinks"]
EXTRA = ["told", "dup_ids", "thinks", "woken", "brain_us_per_mind_tick"]
def sign(x, y):
    up = sum(1 for p, q in zip(x, y) if q > p); dn = sum(1 for p, q in zip(x, y) if q < p); n = up + dn
    if n == 0: return up, dn, 1.0
    z = (abs(up - dn) - 1) / math.sqrt(n)
    return up, dn, min(1.0, math.erfc(max(z, 0) / math.sqrt(2)))
def dz(x, y):
    d = [q - p for p, q in zip(x, y)]; n = len(d)
    if n < 2: return 0.0
    m = sum(d) / n; sd = math.sqrt(sum((v - m) ** 2 for v in d) / (n - 1))
    return m / sd if sd > 0 else 0.0
def holm(ps):
    order = sorted(range(len(ps)), key=lambda i: ps[i]); adj = [1.0] * len(ps); run = 0.0
    for k, i in enumerate(order): run = max(run, min(1.0, (len(ps) - k) * ps[i])); adj[i] = run
    return adj
res = []
for m in modes:
    seeds = sorted(set(by[base]) & set(by.get(m, {})))
    stats = []
    for k in KEYS + EXTRA:
        x = [float(by[base][s][k]) for s in seeds]; y = [float(by[m][s][k]) for s in seeds]
        ma, mb = sum(x) / len(x), sum(y) / len(y)
        up, dn, p = sign(x, y)
        stats.append([k, ma, mb, up, dn, p, dz(x, y)])
    adj = holm([s[5] for s in stats[:len(KEYS)]])
    for i, s in enumerate(stats): s.append(adj[i] if i < len(KEYS) else float('nan'))
    sig = sum(1 for s in stats[:len(KEYS)] if s[7] < 0.05)
    res.append((m, seeds, stats, sig))
for m, seeds, stats, sig in res:
    print(f"\n## {m} against {base}: {len(seeds)} paired valleys; {sig} of {len(KEYS)} measures differ (Holm-corrected sign test, 0.05)")
    if md:
        print("| Measure | " + base + " | " + m + " | Change | Higher / lower | p | p (Holm) | d_z |"); print("|---|---|---|---|---|---|---|---|")
    for k, ma, mb, up, dn, p, d, pa in stats:
        ch = f"{(mb - ma) / ma * 100:+.0f}%" if ma else "–"
        ps = "< 0.001" if p < 0.001 else f"{p:.3f}"; pas = "–" if pa != pa else ("< 0.001" if pa < 0.001 else f"{pa:.3f}")
        if md: print(f"| {k} | {ma:.4g} | {mb:.4g} | {ch} | {up} / {dn} | {ps} | {pas} | {d:+.2f} |")
        else: print(f"{k:28s} {ma:10.4g} {mb:10.4g} {ch:>6s} {up:4d}/{dn:<4d} p {ps:>7s} holm {pas:>7s} dz {d:+.2f}")
if out:
    with open(out, 'w') as f:
        w = csv.writer(f); w.writerow(["mode", "baseline", "measure", "base_mean", "mode_mean", "higher", "lower", "p", "p_holm", "d_z"])
        for m, seeds, stats, sig in res:
            for k, ma, mb, up, dn, p, d, pa in stats: w.writerow([m, base, k, f"{ma:.6g}", f"{mb:.6g}", up, dn, f"{p:.6g}", f"{pa:.6g}" if pa == pa else "", f"{d:.4f}"])
