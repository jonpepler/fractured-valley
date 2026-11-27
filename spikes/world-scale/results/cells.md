## 1. fv-core today (dead tree, 48×48×48 = 110592 cells, 60 ticks)

| | bytes | per cell |
|---|---|---|
| `size_of::<Cell>()` | 136 | 136 |
| held after building the scene | 37555216 | 340 |
| peak during ticks | 83496120 | 755 |

## 2. What the cells are (end of run)

| open air | dense solid | porous solid | open water |
|---|---|---|---|
| 92649 (84 %) | 0 (0 %) | 17943 (16 %) | 0 |

## 3. Widest value of each store over the run (bits needed)

| store | widest | bits |
|---|---|---|
| solid part (µg) | 2650000000 | 32 |
| water (µg) | 209999989 | 28 |
| one gas (µg) | 1914587 | 21 |
| gas energy (µJ) | 789196836 | 30 |
| solid energy (µJ) | 554383763780 | 40 |
| skin (µJ) | 61607634423 | 36 |
| nuclear (µJ) | 0 | 1 |

## 4. Bricks (8³, 216 of them)

| air only | solid, no gas | solid and gas | every cell identical |
|---|---|---|---|
| 162 | 0 | 54 | 0 |

## 5. Bytes per cell, by layout (state only, one copy)

| layout | bytes per cell |
|---|---|
| fv-core `Cell` (array of structs, 64-bit everything) | 136 |
| compact, every field (u32 masses, 64-bit energies) | 80 |
| compact, per-brick field groups (this scene) | 48.0 |

Double-buffered for read-previous/write-own: ×2. Brick index (hash or 2-level table, ~32 B per 512-cell brick): 0.06 B per cell.
Halo: kernels read neighbours through the index, so no copied ghost layer (a copied 1-cell halo on 8³ would be ×1.95).
