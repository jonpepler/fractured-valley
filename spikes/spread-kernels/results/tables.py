#!/usr/bin/env python3
"""Markdown tables from results/bench.csv: python3 results/tables.py > results/tables.md"""
import csv, collections, sys
rows = list(csv.DictReader(open(sys.argv[1] if len(sys.argv) > 1 else 'results/bench.csv')))
order = ['oracle', 'exact-v1', 'exact-fast', 'exact-pruned', 'redef', 'redef-lazy']
for th in ['1', '4']:
    print(f'\n### Steps per second, {th} thread{"s" if th != "1" else ""} (whole machine; per core = this / {th})\n')
    ns = sorted({int(r['n']) for r in rows}); ms = sorted({int(r['m']) for r in rows})
    print('| N | M | ' + ' | '.join(order) + ' |'); print('|' + '---|' * (len(order) + 2))
    for n in ns:
        for m in ms:
            cells = []
            for v in order:
                r = [x for x in rows if x['variant'] == v and int(x['n']) == n and int(x['m']) == m and x['threads'] == (th if v != 'oracle' else '1')]
                cells.append(f"{float(r[0]['steps_per_s'])/1e3:.0f}k" if r else '–')
            if any(c != '–' for c in cells): print(f'| {n:,} | {m:,} | ' + ' | '.join(cells) + ' |')
print('\n### Per step (exact-pruned / redef, 1 thread): µs, edges visited, nodes expanded, state entries, estimated bytes read\n')
print('| N | M | overlay | µs exact-pruned | µs redef | edges (A) | edges (B) | expanded | state bytes | est. bytes read (A) | delta bytes/instance |')
print('|---|---|---|---|---|---|---|---|---|---|---|')
for r in rows:
    if r['variant'] == 'exact-pruned' and r['threads'] == '1':
        b = [x for x in rows if x['variant'] == 'redef' and x['n'] == r['n'] and x['m'] == r['m'] and x['threads'] == '1'][0]
        print(f"| {int(r['n']):,} | {int(r['m']):,} | {r['overlay']} | {float(r['us_per_step_wall']):.2f} | {float(b['us_per_step_wall']):.2f} | {r['edges_visited_per_step']} | {b['edges_visited_per_step']} | {r['nodes_expanded_per_step']} | {r['state_bytes_per_instance']} | {int(r['est_bytes_read_per_step']):,} | {int(r['delta_bytes_per_instance']):,} |")
