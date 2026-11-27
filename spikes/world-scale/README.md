# world-scale: measurements for brief 10

Small, measured spikes behind the world-scale brief (the brief itself lives in the private planning repo). This crate stands alone: it reads `fv-core` as a path dependency and changes nothing in it.

```
cargo run --release --bin cells      # bytes per cell: fv-core today, and compact brick layouts
cargo run --release --bin reflux     # heat across a coarse–fine boundary, exact books (Berger–Colella)
cargo run --release --bin genedits   # a 1 km² valley as generator(seed) + edits
```

Each run's output, as committed, is in `results/`. Machine: 4-core Xeon @ 2.1 GHz cloud VM, one thread unless said otherwise.

## cells

- Counts every byte the process holds with a counting allocator, on the dead-tree scene (48³ = 110 592 cells).
- Records the widest value each store reaches over 60 ticks, to choose lossless narrow widths.
- Prices layouts per 8³ brick, where a brick stores a field group only if one of its cells uses it.

## reflux

- An 8 m cube of stone in 1 m coarse cells. Cells are refined ×10 to 10 cm where the temperature spread exceeds a threshold, with a 1-cell buffer, and regridded every 5 coarse steps.
- The fine level subcycles: 10 fine steps per coarse step.
- It runs four ways: all fine (the reference), adaptive with a flux register (refluxing), adaptive without one, and all coarse.
- Energy is whole µJ and every face flux is antisymmetric, so the only possible leak is at the coarse–fine boundary.
- Refinement prolongs a coarse cell's energy as a limited linear profile, then places the integer remainder so the sum is exact. Coarsening sums the fine cells back.

## genedits

- A 1 km × 1 km × 64 m valley at 10 cm, from integer value noise. It has a meandering floor, a river, soil over stone, ore and grass.
- Counts the bricks that hold the surface, and prices the valley dense, as sparse palette bricks, and as generator + edits.
- Times materialisation per cell, and what streaming costs a player who moves through the valley.
- Builds a canonical (sorted) edit log for a long game: digs, buildings, a 10 ha fire and logs. It reports the log raw and deflated.
- Only materials are edited. The live state of a quiet region goes back to the coarse level, not into the log.
