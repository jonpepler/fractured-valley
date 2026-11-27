### STEPS=4
8 m cube of stone, 1 m coarse cells refined ×10 to 10 cm; 4 coarse steps of 4000 s (40 fine steps of 400 s), 0.2 days.
A 40 cm cube at 1200 K in 290 K stone. Refine where a coarse cell's spread > 2 K (+1 cell buffer), regrid every 5 coarse steps.

| run | energy gap at end (µJ) | worst gap in run (µJ) | coarse-cell temp error, max / mean (mK) | temp at the spot (K) | cell-steps | ms | refined coarse cells (least–most) | regrids that changed the set |
|---|---|---|---|---|---|---|---|---|
| all fine (reference) | 0 | – | 0 / 0 | 643.8 | 20480000 | 177 | 512 | – |
| adaptive, refluxed | 0 | 0 | 0 / 0 | 643.8 | 2562048 | 51 | 64–64 | 0 |
| adaptive, not refluxed | 0 | 0 | 0 / 0 | 643.8 | 2562048 | 51 | 64–64 | 0 |
| all coarse | 0 | 0 | 341 / 11 | 296.9 | 2048 | 0 | 0–0 | 0 |

### STEPS=20
8 m cube of stone, 1 m coarse cells refined ×10 to 10 cm; 20 coarse steps of 4000 s (200 fine steps of 400 s), 0.9 days.
A 40 cm cube at 1200 K in 290 K stone. Refine where a coarse cell's spread > 2 K (+1 cell buffer), regrid every 5 coarse steps.

| run | energy gap at end (µJ) | worst gap in run (µJ) | coarse-cell temp error, max / mean (mK) | temp at the spot (K) | cell-steps | ms | refined coarse cells (least–most) | regrids that changed the set |
|---|---|---|---|---|---|---|---|---|
| all fine (reference) | 0 | – | 0 / 0 | 341.0 | 102400000 | 1225 | 512 | – |
| adaptive, refluxed | 0 | 0 | 0 / 0 | 341.0 | 12810240 | 275 | 64–64 | 0 |
| adaptive, not refluxed | 87817986960 | 87817986960 | 1 / 0 | 341.0 | 12810240 | 308 | 64–64 | 0 |
| all coarse | 0 | 0 | 1161 / 36 | 295.8 | 10240 | 0 | 0–0 | 0 |

### STEPS=60
8 m cube of stone, 1 m coarse cells refined ×10 to 10 cm; 60 coarse steps of 4000 s (600 fine steps of 400 s), 2.8 days.
A 40 cm cube at 1200 K in 290 K stone. Refine where a coarse cell's spread > 2 K (+1 cell buffer), regrid every 5 coarse steps.

| run | energy gap at end (µJ) | worst gap in run (µJ) | coarse-cell temp error, max / mean (mK) | temp at the spot (K) | cell-steps | ms | refined coarse cells (least–most) | regrids that changed the set |
|---|---|---|---|---|---|---|---|---|
| all fine (reference) | 0 | – | 0 / 0 | 300.7 | 307200000 | 3356 | 512 | – |
| adaptive, refluxed | 0 | 0 | 0 / 0 | 300.7 | 76830720 | 1915 | 64–160 | 1 |
| adaptive, not refluxed | 144221393808 | 144221393808 | 1 / 0 | 300.7 | 76830720 | 1731 | 64–160 | 1 |
| all coarse | 0 | 0 | 591 / 18 | 293.8 | 30720 | 0 | 0–0 | 0 |

### STEPS=200
8 m cube of stone, 1 m coarse cells refined ×10 to 10 cm; 200 coarse steps of 4000 s (2000 fine steps of 400 s), 9.3 days.
A 40 cm cube at 1200 K in 290 K stone. Refine where a coarse cell's spread > 2 K (+1 cell buffer), regrid every 5 coarse steps.

| run | energy gap at end (µJ) | worst gap in run (µJ) | coarse-cell temp error, max / mean (mK) | temp at the spot (K) | cell-steps | ms | refined coarse cells (least–most) | regrids that changed the set |
|---|---|---|---|---|---|---|---|---|
| all fine (reference) | 0 | – | 0 / 0 | 291.8 | 1024000000 | 10195 | 512 | – |
| adaptive, refluxed | 0 | 0 | 58 / 4 | 291.4 | 188902400 | 5399 | 0–160 | 3 |
| adaptive, not refluxed | 3539958648408 | 3539958648408 | 59 / 4 | 291.4 | 188902400 | 5056 | 0–160 | 3 |
| all coarse | 0 | 0 | 40 / 8 | 291.4 | 102400 | 1 | 0–0 | 0 |

### STEPS=200 THRESH_MK=10000
8 m cube of stone, 1 m coarse cells refined ×10 to 10 cm; 200 coarse steps of 4000 s (2000 fine steps of 400 s), 9.3 days.
A 40 cm cube at 1200 K in 290 K stone. Refine where a coarse cell's spread > 10 K (+1 cell buffer), regrid every 5 coarse steps.

| run | energy gap at end (µJ) | worst gap in run (µJ) | coarse-cell temp error, max / mean (mK) | temp at the spot (K) | cell-steps | ms | refined coarse cells (least–most) | regrids that changed the set |
|---|---|---|---|---|---|---|---|---|
| all fine (reference) | 0 | – | 0 / 0 | 291.8 | 1024000000 | 9063 | 512 | – |
| adaptive, refluxed | 0 | 0 | 153 / 8 | 291.5 | 38502400 | 1040 | 0–64 | 1 |
| adaptive, not refluxed | 3754586644656 | 3754586644656 | 159 / 9 | 291.5 | 38502400 | 833 | 0–64 | 1 |
| all coarse | 0 | 0 | 40 / 8 | 291.4 | 102400 | 1 | 0–0 | 0 |
