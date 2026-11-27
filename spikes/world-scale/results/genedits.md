## 1. The valley

1 km × 1 km × 64 m at 10 cm: 64.0×10⁹ cells, 125000000 bricks of 8³. Floor 12 m, rim ~37 m, river at 11.8 m.

| | count |
|---|---|
| all bricks | 125000000 |
| surface bricks (span ground level in some column) | 1741746 (1.39 %) |
| check on a 3080-column sample: bricks really mixed solid/air vs estimated | 3044 vs 3438 |
| scanning every column's surface | 964 ms |

## 2. Storing it

| representation | bytes |
|---|---|
| dense, fv-core today (340 B/cell resident) | 20265.58 GiB |
| dense, 1 byte material per cell | 59.60 GiB |
| sparse surface bricks, materials only, per-brick palette (mean 129 B/brick) | 214.17 MiB |
| generator + edits, unedited | 0 (the seed) |

## 3. Generating

Materialising surface bricks: **12.1 ns per cell**, 6.2 µs per brick, one thread (4914176 cells).
- Active radius 32 m, moving 6 m/s: 1869 new bricks/s, **11.6 ms of generation per second** (0.58 ms per 50 ms tick).
- Active radius 64 m, moving 6 m/s: 3738 new bricks/s, **23.2 ms of generation per second** (1.16 ms per 50 ms tick).
- Active radius 64 m, moving 20 m/s: 12459 new bricks/s, **77.4 ms of generation per second** (3.87 ms per 50 ms tick).

## 4. Edits after a long game (cumulative)

| edits | cells changed | bricks touched | log, raw | log, deflated | deflated, per changed cell |
|---|---|---|---|---|---|
| 10 000 digs and placements (~0.5 m³ each) | 3681964 | 66010 | 7.98 MiB | 0.81 MiB | 0.23 B |
| + 40 buildings, 8 × 6 × 4 m | 5436996 | 79949 | 10.60 MiB | 0.90 MiB | 0.17 B |
| + a 10 ha grass fire (surface layer to ash) | 14268241 | 233166 | 29.40 MiB | 2.70 MiB | 0.20 B |
| + 2 000 logs left lying (30 cm × 2 m) | 14623465 | 242751 | 30.38 MiB | 2.80 MiB | 0.20 B |
