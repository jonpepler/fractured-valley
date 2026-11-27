#!/usr/bin/env python3
"""Round 2, stage 3: merge the parts of the rate study (village_study runs and days CSVs written in
several runs) into one, keeping each (seed, mode) once, the first file listed winning.
python3 tools/rate_merge.py <out prefix> <runs.csv> [<runs.csv> ...]   (days files found beside)"""
import csv, sys
out = sys.argv[1]; files = sys.argv[2:]
seen = set(); rows = []; header = None
# A file may be given as path@mode,mode to keep only those modes from it.
def split(f): return (f.split('@')[0], set(f.split('@')[1].split(','))) if '@' in f else (f, None)
for f in files:
    f, keep = split(f)
    for r in csv.DictReader(open(f)):
        if keep and r['mode'] not in keep: continue
        k = (r['seed'], r['mode'])
        if k in seen: continue
        seen.add(k); rows.append(r); header = header or list(r.keys())
rows.sort(key=lambda r: (r['mode'], int(r['seed'])))
with open(out + '.csv', 'w', newline='') as fo:
    w = csv.DictWriter(fo, fieldnames=header, extrasaction='ignore'); w.writeheader(); w.writerows(rows)
from collections import Counter
drows = []; dh = None; dseen = set()
for f in files:
    rf = split(f)[0]
    df = rf.replace('/runs', '/days') if '/runs' in rf else rf[:-4] + '-days.csv'
    for r in csv.DictReader(open(df)):
        k = (r['seed'], r['mode'], r['day'])
        if (r['seed'], r['mode']) not in seen or k in dseen: continue
        dseen.add(k); drows.append(r); dh = dh or list(r.keys())
drows.sort(key=lambda r: (r['mode'], int(r['seed']), int(r['day'])))
with open(out + '-days.csv', 'w', newline='') as fo:
    w = csv.DictWriter(fo, fieldnames=dh); w.writeheader(); w.writerows(drows)
from collections import Counter
cnt = Counter((r['seed'], r['mode']) for r in drows)
short = [k for k in seen if cnt.get(k, 0) == 0]
print("valleys with no day rows:", len(short))
print(len(rows), "valley rows,", len(drows), "day rows;", dict(Counter(r['mode'] for r in rows)))
