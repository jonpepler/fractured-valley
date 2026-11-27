# Mind compaction: the standing baseline

Machine: Intel(R) Xeon(R) Processor @ 2.30GHz, 4 threads; one thread per measurement. Seed `0xf0c0de20261127`, 1500 ticks per size. Cache misses: from the CPU's counters where the machine exposes them (`n/a`: no PMU, as in most VMs). Whole run: 66 s.

Regenerate with `cargo run --release --bin baseline` (in `spikes/mind-compaction`). Every number below is from that run.

**Columns.** *B/node* and *per mind*: structure bytes for one mind (shared parts apart). *State*: bytes kept per mind between thinks. *µs*: per think, mean and p99. *Links*: links read per think. *Agreement* is against the reference over the same 1500 ticks: same top thought, workspace (top-8) overlap, same action on the fixed decision rule, and activation error (Σ|Δ| / Σ reference). In brackets, where different: the tie-aware figure, which also counts a pick the reference ranks equal to its own (integer activations tie often and the reference breaks ties by node id, so a renumbered mind can pick a different, equally ranked node). *Regret*: how much worse the variant's choice is under the reference's own scores, per mille of the reference's best (the decision rule is a close call on many ticks, so *Action* flips on small activation changes while *Regret* stays small). *Exact*: the run's hash equals the reference's.

## Minds

| Nodes | Links | Links/node | Generate + overlay + workload (ms) |
|---|---|---|---|
| 1k (1005) | 9194 | 9.1 | 1 |
| 10k (10050) | 95798 | 9.5 | 12 |
| 100k (100500) | 956658 | 9.5 | 145 |
| 1M (1005000) | 9548844 | 9.5 | 1650 |

## 1k nodes

| Variant | Family | Intent | B/node | Per mind | Shared | State | Build ms | µs mean | µs p99 | Cache misses | Links | Top-1 | WS overlap | Action | Regret ‰ | Act err ‰ | Exact | Note |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| reference graph | baseline | exact | 90.9 | 89.2 KB |  | 8248 B | 0 | 11.4 | 19.3 | n/a | 948 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| packed (u8 weight, no link valence) | baseline | faithful | 62.5 | 61.3 KB |  | 8248 B | 0 | 10.8 | 18.2 | n/a | 948 | 100.0% | 99.3% (99.7%) | 96.4% | 10 | 1 |  |  |
| bit vectors 2048 (whole mind) | baseline | explore | 263.6 | 258.7 KB | 251.2 KB | 8204 B | 19 | 1691 | 2410 | n/a | 783 | 81.6% (100.0%) | 90.6% (91.2%) | 95.0% | 10 | 107 |  | 60 ticks.  |
| swap-in state (active list only) | state | exact | 90.9 | 89.2 KB | 8248 B | 2048 B | 0 | 11.5 | 19.5 | n/a | 948 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| shared base + overlay (COW) | sharing | exact | 1.8 | 1837 B | 88.5 KB | 8248 B | 0 | 15.5 | 24.3 | n/a | 948 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| COW, dense per-mind valence | sharing | exact | 3.8 | 3847 B | 88.5 KB | 8248 B | 0 | 13.1 | 21.2 | n/a | 948 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| weights u16 | precision | faithful | 80.8 | 79.3 KB |  | 8248 B | 0 | 11.9 | 20.3 | n/a | 948 | 100.0% | 99.4% (99.8%) | 97.1% | 12 | 0 |  |  |
| weights u8 (steps of 4) | precision | faithful | 71.6 | 70.3 KB |  | 8248 B | 0 | 11.8 | 20.1 | n/a | 948 | 100.0% | 99.2% (99.7%) | 96.6% | 12 | 1 |  |  |
| weights 4-bit linear | precision | explore | 62.5 | 61.3 KB |  | 8248 B | 0 | 11.6 | 19.8 | n/a | 949 | 100.0% | 97.0% (98.7%) | 88.3% | 39 | 16 |  |  |
| weights 4-bit log | precision | explore | 62.5 | 61.3 KB |  | 8236 B | 0 | 11.6 | 19.4 | n/a | 943 | 99.9% | 96.6% (97.7%) | 87.1% | 39 | 22 |  |  |
| weights 3-bit log | precision | explore | 62.5 | 61.3 KB |  | 8232 B | 0 | 11.3 | 21.3 | n/a | 917 | 97.6% | 88.8% (90.9%) | 76.2% | 99 | 68 |  |  |
| weights 2-bit log | precision | explore | 62.5 | 61.3 KB |  | 8240 B | 0 | 11.7 | 21.2 | n/a | 953 | 99.8% | 88.8% (90.7%) | 72.3% | 80 | 93 |  |  |
| prune links < 50 | pruning | explore | 78.5 | 77.0 KB |  | 8248 B | 0 | 10.8 | 18.2 | n/a | 844 | 99.9% | 98.9% (99.6%) | 96.1% | 7 | 5 |  |  |
| prune links < 100 | pruning | explore | 70.6 | 69.2 KB |  | 8256 B | 0 | 10.3 | 18.0 | n/a | 786 | 99.8% | 98.3% (99.1%) | 92.4% | 16 | 14 |  |  |
| prune links < 200 | pruning | explore | 59.9 | 58.8 KB |  | 8252 B | 0 | 9.6 | 17.3 | n/a | 695 | 98.5% | 93.1% (94.0%) | 83.6% | 61 | 51 |  |  |
| top 16 links per node | pruning | explore | 82.6 | 81.1 KB |  | 8316 B | 0 | 11.4 | 18.9 | n/a | 910 | 100.0% | 95.3% (98.5%) | 85.0% | 63 | 61 |  |  |
| top 8 links per node | pruning | explore | 68.2 | 67.0 KB |  | 8304 B | 0 | 9.3 | 15.8 | n/a | 663 | 99.7% | 92.7% (94.4%) | 71.4% | 25 | 164 |  |  |
| top 4 links per node | pruning | explore | 44.4 | 43.5 KB |  | 8256 B | 0 | 6.3 | 11.4 | n/a | 359 | 91.4% | 78.1% (79.7%) | 58.1% | 87 | 530 |  |  |
| hierarchical, 4 clusters | hierarchy | explore | 97.4 | 95.6 KB |  | 8252 B | 0 | 13.2 | 21.1 | n/a | 879 | 94.0% | 83.5% (89.6%) | 57.7% | 302 | 314 |  |  |
| hierarchical, 16 clusters | hierarchy | explore | 97.4 | 95.6 KB |  | 8252 B | 0 | 13.5 | 23.3 | n/a | 928 | 100.0% | 98.1% (99.4%) | 94.5% | 16 | 8 |  |  |
| hierarchical, 64 clusters | hierarchy | explore | 97.4 | 95.6 KB |  | 8248 B | 0 | 13.6 | 22.6 | n/a | 948 | 100.0% | 100.0% | 100.0% | 0 | 0 |  |  |
| skip unchanged (tolerance 0) | event | explore | 90.9 | 89.2 KB |  | 8248 B | 0 | 12.0 | 20.5 | n/a | 943 | 100.0% | 99.9% (100.0%) | 99.9% | 0 | 0 |  | skipped 6 of 1500 thinks |
| skip unchanged (tolerance 20) | event | explore | 90.9 | 89.2 KB |  | 8252 B | 0 | 6.3 | 20.2 | n/a | 438 | 94.5% | 92.2% (94.5%) | 85.9% | 37 | 96 |  | skipped 794 of 1500 thinks |
| skip unchanged (tolerance 60) | event | explore | 90.9 | 89.2 KB |  | 8260 B | 0 | 5.2 | 20.4 | n/a | 350 | 89.0% | 82.9% (86.6%) | 77.6% | 79 | 221 |  | skipped 936 of 1500 thinks |
| delta input (steady input not re-spread) | event | explore | 90.9 | 89.2 KB |  | 8168 B | 0 | 8.2 | 15.9 | n/a | 608 | 95.5% | 88.9% (90.9%) | 73.4% | 109 | 263 |  |  |
| frontier: select_nth | think | exact | 90.9 | 89.2 KB |  | 8248 B | 0 | 12.5 | 20.7 | n/a | 948 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| frontier: fixed-size heap | think | exact | 90.9 | 89.2 KB |  | 8248 B | 0 | 12.5 | 21.3 | n/a | 948 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| select_nth + gain memo | think | exact | 90.9 | 89.2 KB |  | 8248 B | 0 | 12.2 | 21.1 | n/a | 948 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| select_nth + two-stage gain table | think | exact | 90.9 | 89.2 KB |  | 8248 B | 0 | 11.5 | 18.9 | n/a | 948 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| select_nth + fixed-point flow | think | faithful | 90.9 | 89.2 KB |  | 8248 B | 0 | 11.6 | 19.9 | n/a | 957 | 100.0% | 95.9% (97.9%) | 82.6% | 41 | 6 |  |  |
| select_nth + fixed-point flow and gain | think | faithful | 90.9 | 89.2 KB |  | 8248 B | 0 | 8.8 | 14.2 | n/a | 957 | 100.0% | 95.8% (97.9%) | 82.2% | 41 | 6 |  |  |
| fused think (exact) | think | exact | 91.9 | 90.2 KB |  | 8248 B | 0 | 9.7 | 16.5 | n/a | 948 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| fused think, fixed-point gain | think | faithful | 91.9 | 90.2 KB |  | 8248 B | 0 | 6.7 | 11.2 | n/a | 957 | 100.0% | 95.8% (97.9%) | 82.2% | 41 | 6 |  |  |
| array of structs links | layout | exact | 118.4 | 116.2 KB |  | 8248 B | 0 | 10.8 | 18.5 | n/a | 948 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| shuffled ids (learning order) | layout | up to ties | 90.9 | 89.2 KB |  | 8248 B | 0 | 11.4 | 19.3 | n/a | 948 | 100.0% | 97.3% (100.0%) | 92.6% | 39 | 0 |  |  |
| shuffled, then BFS order | layout | up to ties | 90.9 | 89.2 KB |  | 8248 B | 0 | 11.6 | 20.0 | n/a | 948 | 94.3% (100.0%) | 94.5% (100.0%) | 76.8% | 149 | 0 |  |  |
| shuffled, then RCM order | layout | up to ties | 90.9 | 89.2 KB |  | 8248 B | 0 | 11.6 | 19.6 | n/a | 948 | 99.1% (100.0%) | 96.6% (100.0%) | 91.2% | 38 | 0 |  |  |
| compact: gap-coded, 4-bit log | compact | explore | 28.6 | 28.1 KB |  | 8236 B | 0 | 12.2 | 21.3 | n/a | 943 | 99.9% | 96.6% (97.7%) | 87.1% | 39 | 22 |  |  |
| compact 4-bit, on RCM order | compact | explore | 30.3 | 29.8 KB |  | 8236 B | 0 | 12.4 | 21.4 | n/a | 943 | 99.2% (99.9%) | 94.4% (97.8%) | 80.7% | 74 | 22 |  |  |
| compact: gap-coded, u8 weights | compact | faithful | 37.8 | 37.1 KB |  | 8248 B | 0 | 12.3 | 21.3 | n/a | 948 | 100.0% | 99.2% (99.7%) | 96.6% | 12 | 1 |  |  |
| stack: compact 4-bit + swap + fixed-point | compact | explore | 28.6 | 28.1 KB | 8236 B | 2048 B | 0 | 9.1 | 15.6 | n/a | 951 | 99.9% | 94.7% (96.7%) | 80.4% | 49 | 24 |  |  |
| stack: compact u8 + swap + fixed-point | compact | faithful | 37.8 | 37.1 KB | 8248 B | 2048 B | 0 | 9.0 | 15.4 | n/a | 957 | 100.0% | 95.9% (97.9%) | 82.6% | 40 | 6 |  |  |

## 10k nodes

| Variant | Family | Intent | B/node | Per mind | Shared | State | Build ms | µs mean | µs p99 | Cache misses | Links | Top-1 | WS overlap | Action | Regret ‰ | Act err ‰ | Exact | Note |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| reference graph | baseline | exact | 94.4 | 926.5 KB |  | 78.6 KB | 0 | 10.1 | 19.6 | n/a | 794 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| packed (u8 weight, no link valence) | baseline | faithful | 64.8 | 636.0 KB |  | 78.6 KB | 0 | 9.7 | 19.0 | n/a | 794 | 99.5% | 99.5% (99.7%) | 91.3% | 32 | 1 |  |  |
| bit vectors 2048 (whole mind) | baseline | explore | 263.6 | 2587.2 KB | 2512.5 KB | 78.8 KB | 207 | 36529 | 51417 | n/a | 706 | 100.0% | 88.7% (92.0%) | 50.0% | 144 | 93 |  | 60 ticks.  |
| swap-in state (active list only) | state | exact | 94.4 | 926.5 KB | 78.6 KB | 2048 B | 0 | 9.9 | 19.1 | n/a | 794 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| shared base + overlay (COW) | sharing | exact | 2.1 | 20.6 KB | 918.4 KB | 78.6 KB | 0 | 15.8 | 27.5 | n/a | 794 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| COW, dense per-mind valence | sharing | exact | 4.1 | 40.3 KB | 918.4 KB | 78.6 KB | 0 | 11.9 | 21.6 | n/a | 794 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| weights u16 | precision | faithful | 83.9 | 823.1 KB |  | 78.6 KB | 0 | 9.6 | 18.5 | n/a | 794 | 100.0% | 99.9% | 99.7% | 0 | 0 |  |  |
| weights u8 (steps of 4) | precision | faithful | 74.3 | 729.6 KB |  | 78.6 KB | 0 | 9.7 | 19.2 | n/a | 794 | 99.5% | 99.7% (99.8%) | 99.6% | 0 | 1 |  |  |
| weights 4-bit linear | precision | explore | 64.8 | 636.0 KB |  | 78.6 KB | 2 | 9.5 | 18.6 | n/a | 791 | 98.7% (98.8%) | 97.0% (97.9%) | 98.6% | 4 | 17 |  |  |
| weights 4-bit log | precision | explore | 64.8 | 636.0 KB |  | 78.6 KB | 2 | 9.4 | 18.4 | n/a | 784 | 98.3% (98.6%) | 96.1% (96.9%) | 97.1% | 4 | 26 |  |  |
| weights 3-bit log | precision | explore | 64.8 | 636.0 KB |  | 78.6 KB | 1 | 9.4 | 19.0 | n/a | 784 | 97.0% (97.4%) | 92.4% (93.5%) | 95.1% | 5 | 62 |  |  |
| weights 2-bit log | precision | explore | 64.8 | 636.0 KB |  | 78.6 KB | 1 | 9.3 | 18.9 | n/a | 789 | 98.2% (98.5%) | 91.0% (92.1%) | 86.5% | 23 | 85 |  |  |
| prune links < 50 | pruning | explore | 81.2 | 797.2 KB |  | 78.6 KB | 1 | 8.9 | 17.5 | n/a | 716 | 99.2% | 99.3% (99.6%) | 99.8% | 0 | 6 |  |  |
| prune links < 100 | pruning | explore | 72.7 | 713.5 KB |  | 78.6 KB | 1 | 8.4 | 17.4 | n/a | 661 | 94.9% (95.2%) | 98.3% (98.6%) | 98.4% | 1 | 17 |  |  |
| prune links < 200 | pruning | explore | 61.6 | 604.7 KB |  | 78.6 KB | 1 | 7.9 | 16.3 | n/a | 590 | 94.3% (94.6%) | 97.2% (97.8%) | 97.4% | 3 | 43 |  |  |
| top 16 links per node | pruning | explore | 84.3 | 827.6 KB |  | 78.6 KB | 3 | 8.7 | 17.5 | n/a | 695 | 99.7% (99.9%) | 98.2% (98.4%) | 90.4% | 74 | 20 |  |  |
| top 8 links per node | pruning | explore | 68.7 | 674.6 KB |  | 78.6 KB | 3 | 7.0 | 14.6 | n/a | 502 | 94.6% | 94.7% (95.6%) | 84.7% | 130 | 87 |  |  |
| top 4 links per node | pruning | explore | 44.5 | 436.8 KB |  | 78.5 KB | 2 | 5.1 | 11.4 | n/a | 284 | 87.6% | 82.1% (82.8%) | 77.4% | 179 | 372 |  |  |
| hierarchical, 4 clusters | hierarchy | explore | 104.2 | 1023.2 KB |  | 78.6 KB | 0 | 10.3 | 18.1 | n/a | 555 | 81.5% (81.8%) | 83.0% (83.3%) | 81.7% | 162 | 300 |  |  |
| hierarchical, 16 clusters | hierarchy | explore | 104.2 | 1023.2 KB |  | 78.6 KB | 0 | 13.5 | 26.6 | n/a | 714 | 94.7% (95.0%) | 91.8% (93.6%) | 99.6% | 0 | 125 |  |  |
| hierarchical, 64 clusters | hierarchy | explore | 104.2 | 1023.2 KB |  | 78.6 KB | 0 | 14.0 | 27.2 | n/a | 760 | 99.6% | 99.5% (99.6%) | 99.9% | 0 | 9 |  |  |
| skip unchanged (tolerance 0) | event | explore | 94.4 | 926.5 KB |  | 78.6 KB | 0 | 9.9 | 19.2 | n/a | 794 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes | skipped 0 of 1500 thinks |
| skip unchanged (tolerance 20) | event | explore | 94.4 | 926.5 KB |  | 78.6 KB | 0 | 5.5 | 18.9 | n/a | 387 | 88.7% (89.0%) | 93.1% (93.9%) | 98.1% | 0 | 100 |  | skipped 748 of 1500 thinks |
| skip unchanged (tolerance 60) | event | explore | 94.4 | 926.5 KB |  | 78.6 KB | 0 | 4.6 | 19.8 | n/a | 306 | 80.4% | 85.1% (85.5%) | 96.8% | 1 | 229 |  | skipped 899 of 1500 thinks |
| delta input (steady input not re-spread) | event | explore | 94.4 | 926.5 KB |  | 78.5 KB | 0 | 7.2 | 15.2 | n/a | 529 | 85.2% (85.3%) | 90.2% (91.2%) | 97.8% | 1 | 230 |  |  |
| frontier: select_nth | think | exact | 94.4 | 926.5 KB |  | 78.6 KB | 0 | 10.3 | 19.8 | n/a | 794 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| frontier: fixed-size heap | think | exact | 94.4 | 926.5 KB |  | 78.6 KB | 0 | 10.7 | 20.1 | n/a | 794 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| select_nth + gain memo | think | exact | 94.4 | 926.5 KB |  | 78.6 KB | 0 | 10.4 | 20.2 | n/a | 794 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| select_nth + two-stage gain table | think | exact | 94.4 | 926.5 KB |  | 78.6 KB | 0 | 9.5 | 18.1 | n/a | 794 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| select_nth + fixed-point flow | think | faithful | 94.4 | 926.5 KB |  | 78.6 KB | 0 | 9.6 | 18.6 | n/a | 802 | 99.2% | 98.3% (99.3%) | 100.0% | 0 | 6 |  |  |
| select_nth + fixed-point flow and gain | think | faithful | 94.4 | 926.5 KB |  | 78.6 KB | 0 | 7.4 | 13.9 | n/a | 802 | 99.2% | 98.3% (99.3%) | 100.0% | 0 | 6 |  |  |
| fused think (exact) | think | exact | 95.4 | 936.3 KB |  | 78.6 KB | 0 | 8.1 | 16.2 | n/a | 794 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| fused think, fixed-point gain | think | faithful | 95.4 | 936.3 KB |  | 78.6 KB | 0 | 5.8 | 11.1 | n/a | 802 | 99.2% | 98.3% (99.3%) | 100.0% | 0 | 6 |  |  |
| array of structs links | layout | exact | 123.0 | 1207.2 KB |  | 78.6 KB | 0 | 9.0 | 17.7 | n/a | 794 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| shuffled ids (learning order) | layout | up to ties | 94.4 | 926.5 KB |  | 78.6 KB | 4 | 9.8 | 19.2 | n/a | 794 | 98.8% (100.0%) | 98.4% (100.0%) | 100.0% | 0 | 0 |  |  |
| shuffled, then BFS order | layout | up to ties | 94.4 | 926.5 KB |  | 78.6 KB | 8 | 9.8 | 19.6 | n/a | 794 | 98.8% (100.0%) | 97.6% (100.0%) | 100.0% | 0 | 0 |  |  |
| shuffled, then RCM order | layout | up to ties | 94.4 | 926.5 KB |  | 78.6 KB | 12 | 9.7 | 19.0 | n/a | 794 | 99.6% (100.0%) | 97.5% (100.0%) | 100.0% | 0 | 0 |  |  |
| compact: gap-coded, 4-bit log | compact | explore | 30.3 | 297.2 KB |  | 78.6 KB | 1 | 10.0 | 20.2 | n/a | 784 | 98.3% (98.6%) | 96.1% (96.9%) | 97.1% | 4 | 26 |  |  |
| compact 4-bit, on RCM order | compact | explore | 34.4 | 337.7 KB |  | 78.6 KB | 6 | 10.3 | 20.4 | n/a | 783 | 98.2% (98.6%) | 94.5% (97.0%) | 97.0% | 4 | 26 |  |  |
| compact: gap-coded, u8 weights | compact | faithful | 39.8 | 390.8 KB |  | 78.6 KB | 0 | 10.1 | 19.8 | n/a | 794 | 99.5% | 99.7% (99.8%) | 99.6% | 0 | 1 |  |  |
| stack: compact 4-bit + swap + fixed-point | compact | explore | 30.3 | 297.2 KB | 78.6 KB | 2048 B | 1 | 7.7 | 14.8 | n/a | 792 | 98.2% (98.6%) | 95.8% (96.9%) | 97.1% | 4 | 28 |  |  |
| stack: compact u8 + swap + fixed-point | compact | faithful | 39.8 | 390.8 KB | 78.6 KB | 2048 B | 0 | 7.8 | 15.1 | n/a | 802 | 99.2% | 98.3% (99.3%) | 99.6% | 0 | 6 |  |  |

## 100k nodes

| Variant | Family | Intent | B/node | Per mind | Shared | State | Build ms | µs mean | µs p99 | Cache misses | Links | Top-1 | WS overlap | Action | Regret ‰ | Act err ‰ | Exact | Note |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| reference graph | baseline | exact | 94.3 | 9251.5 KB |  | 785.3 KB | 0 | 10.7 | 20.0 | n/a | 887 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| packed (u8 weight, no link valence) | baseline | faithful | 64.7 | 6350.7 KB |  | 785.2 KB | 5 | 10.1 | 18.8 | n/a | 886 | 99.9% (100.0%) | 99.5% (99.7%) | 99.6% | 0 | 1 |  |  |
| swap-in state (active list only) | state | exact | 94.3 | 9251.5 KB | 785.3 KB | 2048 B | 0 | 10.8 | 20.3 | n/a | 887 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| shared base + overlay (COW) | sharing | exact | 2.0 | 196.8 KB | 9172.8 KB | 785.3 KB | 0 | 20.8 | 36.0 | n/a | 887 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| COW, dense per-mind valence | sharing | exact | 4.0 | 393.1 KB | 9172.8 KB | 785.3 KB | 0 | 14.1 | 25.4 | n/a | 887 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| weights u16 | precision | faithful | 83.7 | 8219.2 KB |  | 785.3 KB | 7 | 10.3 | 19.4 | n/a | 887 | 100.0% | 99.9% | 99.8% | 0 | 0 |  |  |
| weights u8 (steps of 4) | precision | faithful | 74.2 | 7284.9 KB |  | 785.3 KB | 6 | 10.5 | 19.8 | n/a | 886 | 99.9% (100.0%) | 99.7% (99.9%) | 99.8% | 0 | 1 |  |  |
| weights 4-bit linear | precision | explore | 64.7 | 6350.7 KB |  | 785.3 KB | 23 | 10.3 | 19.6 | n/a | 886 | 99.9% (100.0%) | 97.7% (98.2%) | 99.2% | 1 | 18 |  |  |
| weights 4-bit log | precision | explore | 64.7 | 6350.7 KB |  | 785.2 KB | 22 | 10.5 | 19.8 | n/a | 884 | 99.8% | 97.5% (97.9%) | 98.9% | 2 | 27 |  |  |
| weights 3-bit log | precision | explore | 64.7 | 6350.7 KB |  | 785.3 KB | 17 | 10.5 | 19.5 | n/a | 895 | 97.1% (97.2%) | 94.0% (94.6%) | 97.8% | 10 | 58 |  |  |
| weights 2-bit log | precision | explore | 64.7 | 6350.7 KB |  | 785.3 KB | 12 | 10.4 | 19.2 | n/a | 879 | 98.4% (98.5%) | 90.5% (91.0%) | 95.3% | 28 | 82 |  |  |
| prune links < 50 | pruning | explore | 80.9 | 7941.3 KB |  | 785.3 KB | 18 | 9.5 | 18.2 | n/a | 772 | 99.9% (100.0%) | 99.4% (99.7%) | 99.6% | 0 | 8 |  |  |
| prune links < 100 | pruning | explore | 72.3 | 7098.5 KB |  | 785.3 KB | 15 | 9.1 | 17.8 | n/a | 724 | 99.9% (100.0%) | 98.7% (99.1%) | 98.9% | 1 | 19 |  |  |
| prune links < 200 | pruning | explore | 61.2 | 6010.4 KB |  | 785.3 KB | 14 | 8.4 | 16.5 | n/a | 632 | 99.5% (99.6%) | 97.7% (98.1%) | 97.4% | 6 | 52 |  |  |
| top 16 links per node | pruning | explore | 84.3 | 8271.8 KB |  | 785.3 KB | 41 | 9.2 | 19.4 | n/a | 710 | 99.6% (99.7%) | 99.7% (99.8%) | 97.7% | 7 | 9 |  |  |
| top 8 links per node | pruning | explore | 68.7 | 6744.6 KB |  | 785.3 KB | 38 | 7.1 | 15.0 | n/a | 499 | 98.8% (98.9%) | 94.3% (95.2%) | 93.6% | 32 | 146 |  |  |
| top 4 links per node | pruning | explore | 44.5 | 4365.9 KB |  | 785.2 KB | 29 | 4.9 | 11.6 | n/a | 268 | 96.6% | 81.7% (82.6%) | 79.4% | 168 | 377 |  |  |
| hierarchical, 4 clusters | hierarchy | explore | 107.4 | 10.3 MB |  | 785.3 KB | 10 | 12.2 | 21.0 | n/a | 521 | 78.2% | 89.4% (89.6%) | 98.5% | 4 | 227 |  |  |
| hierarchical, 16 clusters | hierarchy | explore | 107.4 | 10.3 MB |  | 785.3 KB | 10 | 16.4 | 30.6 | n/a | 713 | 94.5% | 87.0% (87.7%) | 81.6% | 176 | 248 |  |  |
| hierarchical, 64 clusters | hierarchy | explore | 107.4 | 10.3 MB |  | 785.3 KB | 10 | 18.0 | 36.6 | n/a | 732 | 99.6% | 96.5% (96.6%) | 99.0% | 2 | 66 |  |  |
| skip unchanged (tolerance 0) | event | explore | 94.3 | 9251.5 KB |  | 785.3 KB | 0 | 10.8 | 20.3 | n/a | 885 | 100.0% | 100.0% | 100.0% | 0 | 0 |  | skipped 2 of 1500 thinks |
| skip unchanged (tolerance 20) | event | explore | 94.3 | 9251.5 KB |  | 785.3 KB | 0 | 6.2 | 20.4 | n/a | 451 | 95.5% (95.6%) | 93.9% (94.3%) | 98.7% | 4 | 107 |  | skipped 732 of 1500 thinks |
| skip unchanged (tolerance 60) | event | explore | 94.3 | 9251.5 KB |  | 785.3 KB | 0 | 4.9 | 20.2 | n/a | 344 | 86.7% (86.8%) | 85.4% (85.6%) | 98.2% | 9 | 245 |  | skipped 902 of 1500 thinks |
| delta input (steady input not re-spread) | event | explore | 94.3 | 9251.5 KB |  | 785.2 KB | 0 | 7.6 | 16.2 | n/a | 567 | 87.4% | 93.7% (94.1%) | 98.7% | 2 | 224 |  |  |
| frontier: select_nth | think | exact | 94.3 | 9251.5 KB |  | 785.3 KB | 0 | 11.3 | 20.9 | n/a | 887 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| frontier: fixed-size heap | think | exact | 94.3 | 9251.5 KB |  | 785.3 KB | 0 | 11.4 | 21.6 | n/a | 887 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| select_nth + gain memo | think | exact | 94.3 | 9251.5 KB |  | 785.3 KB | 0 | 11.2 | 21.7 | n/a | 887 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| select_nth + two-stage gain table | think | exact | 94.3 | 9251.5 KB |  | 785.3 KB | 0 | 10.5 | 19.4 | n/a | 887 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| select_nth + fixed-point flow | think | faithful | 94.3 | 9251.5 KB |  | 785.3 KB | 0 | 10.4 | 19.4 | n/a | 895 | 99.3% | 99.2% (99.6%) | 99.8% | 0 | 5 |  |  |
| select_nth + fixed-point flow and gain | think | faithful | 94.3 | 9251.5 KB |  | 785.3 KB | 0 | 8.0 | 14.8 | n/a | 895 | 99.3% | 99.2% (99.6%) | 99.7% | 0 | 5 |  |  |
| fused think (exact) | think | exact | 95.3 | 9349.7 KB |  | 785.3 KB | 1 | 9.1 | 17.4 | n/a | 887 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| fused think, fixed-point gain | think | faithful | 95.3 | 9349.7 KB |  | 785.3 KB | 1 | 6.2 | 11.9 | n/a | 895 | 99.3% | 99.2% (99.6%) | 99.7% | 0 | 5 |  |  |
| array of structs links | layout | exact | 122.8 | 11.8 MB |  | 785.3 KB | 1 | 9.8 | 18.4 | n/a | 887 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| shuffled ids (learning order) | layout | up to ties | 94.3 | 9251.5 KB |  | 785.3 KB | 51 | 10.9 | 21.1 | n/a | 887 | 99.6% (100.0%) | 98.3% (100.0%) | 99.6% | 0 | 0 |  |  |
| shuffled, then BFS order | layout | up to ties | 94.3 | 9251.5 KB |  | 785.3 KB | 119 | 10.8 | 20.2 | n/a | 887 | 99.6% (100.0%) | 98.2% (100.0%) | 99.6% | 0 | 0 |  |  |
| shuffled, then RCM order | layout | up to ties | 94.3 | 9251.5 KB |  | 785.3 KB | 117 | 11.1 | 21.0 | n/a | 887 | 97.0% (100.0%) | 98.4% (100.0%) | 99.3% | 6 | 0 |  |  |
| compact: gap-coded, 4-bit log | compact | explore | 31.8 | 3116.8 KB |  | 785.2 KB | 14 | 11.2 | 21.6 | n/a | 884 | 99.8% | 97.5% (97.9%) | 98.9% | 2 | 27 |  |  |
| compact 4-bit, on RCM order | compact | explore | 38.9 | 3819.5 KB |  | 785.2 KB | 82 | 11.7 | 23.1 | n/a | 884 | 99.4% (99.8%) | 95.7% (97.5%) | 98.6% | 3 | 27 |  |  |
| compact: gap-coded, u8 weights | compact | faithful | 41.3 | 4051.0 KB |  | 785.3 KB | 9 | 11.2 | 21.7 | n/a | 886 | 99.9% (100.0%) | 99.7% (99.9%) | 99.8% | 0 | 1 |  |  |
| stack: compact 4-bit + swap + fixed-point | compact | explore | 31.8 | 3116.8 KB | 785.2 KB | 2048 B | 14 | 8.6 | 16.2 | n/a | 893 | 99.2% | 97.3% (97.9%) | 98.8% | 3 | 29 |  |  |
| stack: compact u8 + swap + fixed-point | compact | faithful | 41.3 | 4051.0 KB | 785.3 KB | 2048 B | 9 | 8.7 | 16.2 | n/a | 895 | 99.3% | 99.2% (99.6%) | 99.6% | 0 | 6 |  |  |

## 1M nodes

| Variant | Family | Intent | B/node | Per mind | Shared | State | Build ms | µs mean | µs p99 | Cache misses | Links | Top-1 | WS overlap | Action | Regret ‰ | Act err ‰ | Exact | Note |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| reference graph | baseline | exact | 94.1 | 90.2 MB |  | 7851.8 KB | 0 | 16.1 | 26.0 | n/a | 1304 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| packed (u8 weight, no link valence) | baseline | faithful | 64.6 | 61.9 MB |  | 7851.8 KB | 77 | 15.2 | 24.6 | n/a | 1304 | 100.0% | 98.8% (99.2%) | 91.6% (92.1%) | 19 | 1 |  |  |
| swap-in state (active list only) | state | exact | 94.1 | 90.2 MB | 7851.8 KB | 2048 B | 0 | 16.4 | 27.1 | n/a | 1304 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| shared base + overlay (COW) | sharing | exact | 2.1 | 2013.5 KB | 89.4 MB | 7851.8 KB | 1 | 43.2 | 58.3 | n/a | 1304 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| COW, dense per-mind valence | sharing | exact | 4.1 | 3976.4 KB | 89.4 MB | 7851.8 KB | 1 | 24.2 | 36.9 | n/a | 1304 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| weights u16 | precision | faithful | 83.6 | 80.1 MB |  | 7851.8 KB | 96 | 15.8 | 25.2 | n/a | 1304 | 100.0% | 99.5% (99.7%) | 98.4% (98.8%) | 1 | 0 |  |  |
| weights u8 (steps of 4) | precision | faithful | 74.1 | 71.0 MB |  | 7851.8 KB | 85 | 15.8 | 25.9 | n/a | 1306 | 100.0% | 99.1% (99.6%) | 93.5% (94.6%) | 4 | 1 |  |  |
| weights 4-bit linear | precision | explore | 64.6 | 61.9 MB |  | 7851.8 KB | 265 | 15.5 | 25.0 | n/a | 1300 | 99.8% | 96.3% (97.9%) | 79.2% | 27 | 15 |  |  |
| weights 4-bit log | precision | explore | 64.6 | 61.9 MB |  | 7851.8 KB | 244 | 15.4 | 24.7 | n/a | 1307 | 99.9% | 94.2% (96.3%) | 90.7% (91.5%) | 17 | 21 |  |  |
| weights 3-bit log | precision | explore | 64.6 | 61.9 MB |  | 7851.8 KB | 176 | 15.3 | 24.7 | n/a | 1276 | 99.8% | 91.7% (94.8%) | 80.3% | 55 | 62 |  |  |
| weights 2-bit log | precision | explore | 64.6 | 61.9 MB |  | 7852.0 KB | 140 | 15.6 | 25.2 | n/a | 1291 | 99.8% | 89.4% (92.9%) | 72.2% | 67 | 97 |  |  |
| prune links < 50 | pruning | explore | 80.8 | 77.5 MB |  | 7851.8 KB | 258 | 14.4 | 23.7 | n/a | 1165 | 100.0% | 97.9% (99.0%) | 95.7% (96.1%) | 5 | 6 |  |  |
| prune links < 100 | pruning | explore | 72.2 | 69.2 MB |  | 7851.8 KB | 242 | 13.8 | 22.3 | n/a | 1081 | 100.0% | 96.9% (98.4%) | 95.0% (95.5%) | 9 | 17 |  |  |
| prune links < 200 | pruning | explore | 61.2 | 58.6 MB |  | 7851.8 KB | 246 | 12.8 | 21.0 | n/a | 965 | 99.4% | 96.1% (97.4%) | 90.4% (90.9%) | 11 | 38 |  |  |
| top 16 links per node | pruning | explore | 84.1 | 80.6 MB |  | 7851.9 KB | 562 | 14.4 | 21.8 | n/a | 1101 | 100.0% | 97.3% (98.4%) | 94.6% | 8 | 69 |  |  |
| top 8 links per node | pruning | explore | 68.7 | 65.8 MB |  | 7851.9 KB | 493 | 11.7 | 17.8 | n/a | 816 | 100.0% | 86.1% (92.4%) | 61.5% | 213 | 345 |  |  |
| top 4 links per node | pruning | explore | 44.5 | 42.6 MB |  | 7851.8 KB | 369 | 7.9 | 12.7 | n/a | 445 | 99.3% | 69.8% (71.2%) | 28.6% | 588 | 738 |  |  |
| hierarchical, 4 clusters | hierarchy | explore | 107.8 | 103.3 MB |  | 7851.7 KB | 145 | 11.8 | 20.5 | n/a | 449 | 10.4% | 70.1% (70.9%) | 72.2% | 63 | 586 |  |  |
| hierarchical, 16 clusters | hierarchy | explore | 107.8 | 103.3 MB |  | 7851.8 KB | 168 | 23.6 | 33.4 | n/a | 882 | 81.7% | 61.4% | 12.5% | 869 | 880 |  |  |
| hierarchical, 64 clusters | hierarchy | explore | 107.8 | 103.3 MB |  | 7851.9 KB | 161 | 32.0 | 43.6 | n/a | 1139 | 99.4% | 94.1% (95.3%) | 89.7% | 21 | 127 |  |  |
| skip unchanged (tolerance 0) | event | explore | 94.1 | 90.2 MB |  | 7851.8 KB | 0 | 16.0 | 25.8 | n/a | 1295 | 100.0% | 99.9% (100.0%) | 100.0% | 0 | 0 |  | skipped 11 of 1500 thinks |
| skip unchanged (tolerance 20) | event | explore | 94.1 | 90.2 MB |  | 7851.8 KB | 0 | 8.1 | 24.7 | n/a | 584 | 99.2% | 90.3% (94.1%) | 88.2% (89.0%) | 27 | 83 |  | skipped 835 of 1500 thinks |
| skip unchanged (tolerance 60) | event | explore | 94.1 | 90.2 MB |  | 7851.8 KB | 0 | 6.8 | 24.5 | n/a | 484 | 98.8% | 85.1% (87.8%) | 85.2% (86.3%) | 27 | 179 |  | skipped 950 of 1500 thinks |
| delta input (steady input not re-spread) | event | explore | 94.1 | 90.2 MB |  | 7851.9 KB | 0 | 11.4 | 20.5 | n/a | 882 | 96.4% | 85.9% (88.0%) | 68.4% | 62 | 313 |  |  |
| frontier: select_nth | think | exact | 94.1 | 90.2 MB |  | 7851.8 KB | 0 | 17.6 | 26.6 | n/a | 1304 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| frontier: fixed-size heap | think | exact | 94.1 | 90.2 MB |  | 7851.8 KB | 0 | 17.8 | 27.5 | n/a | 1304 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| select_nth + gain memo | think | exact | 94.1 | 90.2 MB |  | 7851.8 KB | 0 | 17.6 | 27.4 | n/a | 1304 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| select_nth + two-stage gain table | think | exact | 94.1 | 90.2 MB |  | 7851.8 KB | 0 | 16.3 | 24.6 | n/a | 1304 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| select_nth + fixed-point flow | think | faithful | 94.1 | 90.2 MB |  | 7851.8 KB | 0 | 16.2 | 24.6 | n/a | 1314 | 100.0% | 93.6% (97.4%) | 86.2% | 51 | 7 |  |  |
| select_nth + fixed-point flow and gain | think | faithful | 94.1 | 90.2 MB |  | 7851.8 KB | 0 | 12.7 | 17.8 | n/a | 1314 | 100.0% | 93.4% (97.3%) | 86.3% | 49 | 7 |  |  |
| fused think (exact) | think | exact | 95.1 | 91.2 MB |  | 7852.0 KB | 29 | 14.1 | 22.2 | n/a | 1304 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| fused think, fixed-point gain | think | faithful | 95.1 | 91.2 MB |  | 7852.0 KB | 39 | 9.9 | 14.9 | n/a | 1314 | 100.0% | 93.4% (97.3%) | 86.3% | 49 | 7 |  |  |
| array of structs links | layout | exact | 122.6 | 117.5 MB |  | 7851.8 KB | 50 | 15.4 | 23.5 | n/a | 1304 | 100.0% | 100.0% | 100.0% | 0 | 0 | yes |  |
| shuffled ids (learning order) | layout | up to ties | 94.1 | 90.2 MB |  | 7851.8 KB | 724 | 17.4 | 28.7 | n/a | 1304 | 100.0% | 90.9% (100.0%) | 83.1% | 95 | 0 |  |  |
| shuffled, then BFS order | layout | up to ties | 94.1 | 90.2 MB |  | 7851.8 KB | 1635 | 16.9 | 27.2 | n/a | 1305 | 100.0% | 90.4% (100.0%) | 74.6% (75.8%) | 33 | 0 |  |  |
| shuffled, then RCM order | layout | up to ties | 94.1 | 90.2 MB |  | 7851.8 KB | 2064 | 16.6 | 30.8 | n/a | 1305 | 100.0% | 93.6% (100.0%) | 98.6% | 8 | 0 |  |  |
| compact: gap-coded, 4-bit log | compact | explore | 32.6 | 31.2 MB |  | 7851.8 KB | 150 | 16.9 | 28.0 | n/a | 1307 | 99.9% | 94.2% (96.3%) | 90.7% (91.5%) | 17 | 21 |  |  |
| compact 4-bit, on RCM order | compact | explore | 42.7 | 40.9 MB |  | 7851.8 KB | 1335 | 18.3 | 32.7 | n/a | 1307 | 99.9% | 89.0% (96.0%) | 84.6% (85.4%) | 38 | 21 |  |  |
| compact: gap-coded, u8 weights | compact | faithful | 42.1 | 40.3 MB |  | 7851.8 KB | 103 | 17.1 | 28.1 | n/a | 1306 | 100.0% | 99.1% (99.6%) | 93.5% (94.6%) | 4 | 1 |  |  |
| stack: compact 4-bit + swap + fixed-point | compact | explore | 32.6 | 31.2 MB | 7851.8 KB | 2048 B | 152 | 13.1 | 21.3 | n/a | 1319 | 99.9% | 92.5% (95.9%) | 85.8% (86.4%) | 59 | 22 |  |  |
| stack: compact u8 + swap + fixed-point | compact | faithful | 42.1 | 40.3 MB | 7851.8 KB | 2048 B | 103 | 13.2 | 22.0 | n/a | 1316 | 100.0% | 93.7% (97.4%) | 82.4% (82.8%) | 50 | 7 |  |  |

## Sharing a species base

One species base, many individuals. *Overlay*: link-level copy-on-write (reweights 6 B, added links 13 B, revalues 6 B, new nodes 3 B). *Page COW*: the same changes, copying every touched 64-node page whole. *Alone*: the individual as its own graph.

| Nodes | Minds | Individual differs by | Base (shared) | Overlay per extra mind (mean, max) | Page COW per mind | Alone per mind | Total, shared | Total, alone |
|---|---|---|---|---|---|---|---|---|
| 1k | 100 | 2% links, 1% nodes (default) | 88.5 KB | 2077 B, 2308 B | 86.7 KB | 89.2 KB | 291.4 KB | 8924.3 KB |
| 1k | 1000 | 2% links, 1% nodes (default) | 88.5 KB | 2068 B, 2348 B | 86.6 KB | 89.2 KB | 2108.1 KB | 87.2 MB |
| 1k | 100 | 0.5% links (young) | 88.5 KB | 487 B, 531 B | 62.8 KB | 89.2 KB | 136.1 KB | 8924.3 KB |
| 1k | 100 | 8% links (old) | 88.5 KB | 7197 B, 8714 B | 87.7 KB | 89.2 KB | 791.4 KB | 8924.3 KB |
| 10k | 100 | 2% links, 1% nodes (default) | 918.4 KB | 19.9 KB, 22.5 KB | 900.0 KB | 926.5 KB | 2911.4 KB | 90.5 MB |
| 10k | 1000 | 2% links, 1% nodes (default) | 918.4 KB | 19.9 KB, 22.5 KB | 900.4 KB | 926.5 KB | 20.3 MB | 904.8 MB |
| 10k | 100 | 0.5% links (young) | 918.4 KB | 4993 B, 5308 B | 656.6 KB | 926.5 KB | 1406.0 KB | 90.5 MB |
| 10k | 100 | 8% links (old) | 918.4 KB | 67.3 KB, 80.6 KB | 909.9 KB | 926.5 KB | 7650.9 KB | 90.5 MB |
| 100k | 100 | 2% links, 1% nodes (default) | 9172.8 KB | 199.7 KB, 209.4 KB | 8998.8 KB | 9251.5 KB | 28.5 MB | 903.5 MB |
| 100k | 1000 | 2% links, 1% nodes (default) | 9172.8 KB | 200.2 KB, 211.3 KB | 8999.4 KB | 9251.5 KB | 204.5 MB | 9034.7 MB |
| 100k | 100 | 0.5% links (young) | 9172.8 KB | 49.0 KB, 50.3 KB | 6573.4 KB | 9251.5 KB | 13.7 MB | 903.5 MB |
| 100k | 100 | 8% links (old) | 9172.8 KB | 673.2 KB, 716.8 KB | 9089.5 KB | 9251.5 KB | 74.7 MB | 903.5 MB |
| 1M | 100 | 2% links, 1% nodes (default) | 89.4 MB | 2008.9 KB, 2034.8 KB | 87.7 MB | 90.2 MB | 285.6 MB | 9020.1 MB |
| 1M | 1000 | 2% links, 1% nodes (default) | 89.4 MB | 2008.1 KB, 2049.0 KB | 87.7 MB | 90.2 MB | 2050.4 MB | 90200.7 MB |
| 1M | 100 | 0.5% links (young) | 89.4 MB | 491.6 KB, 494.7 KB | 64.1 MB | 90.2 MB | 137.4 MB | 9020.1 MB |
| 1M | 100 | 8% links (old) | 89.4 MB | 6786.1 KB, 6912.4 KB | 88.6 MB | 90.2 MB | 752.1 MB | 9020.1 MB |

## Fast weights and locality

*Fast-weight links*: distinct links between co-active workspace nodes over the whole run (what a day's fast weights touch). Dense fast weights cost 2 B on every link. *Mean link gap*: mean |source − target| id distance.

| Nodes | Links | Fast-weight links touched | Dense (2 B/link) | Sparse (8 B/entry) | Mean link gap: generated | shuffled | shuffled → RCM |
|---|---|---|---|---|---|---|---|
| 1k | 9194 | 1017 | 18.0 KB | 8136 B | 111 | 337 | 265 |
| 10k | 95798 | 1262 | 187.1 KB | 10096 B | 695 | 3335 | 2754 |
| 100k | 956658 | 874 | 1868.5 KB | 6992 B | 6110 | 33279 | 28740 |
| 1M | 9548844 | 483 | 18.2 MB | 3864 B | 59333 | 332690 | 294273 |

## Bit vectors where they might win: recognition

*Identify*: the percept's thing is found. *Category*: its category (of 8). *Food?*: food or not.

| Things | Method | Bytes | ns per query | Identify | Category | Food? |
|---|---|---|---|---|---|---|
| 200 | graph: inverted index (identify, its category) | 11.2 KB | 450 | 100.0% | 100.0% | 100.0% |
| 200 | graph: features → category (categorise only) | 16.0 KB | 26 |  | 99.4% | 100.0% |
| 200 | bit vectors 256: nearest signature (identify) | 6600 B | 4772 | 98.2% | 99.2% | 100.0% |
| 200 | bit vectors 256: nearest prototype (categorise only) | 256 B | 3545 |  | 92.0% | 98.0% |
| 200 | bit vectors 1024: nearest signature (identify) | 25.2 KB | 16973 | 98.8% | 99.4% | 100.0% |
| 200 | bit vectors 1024: nearest prototype (categorise only) | 1024 B | 13590 |  | 97.4% | 99.4% |
| 1200 | graph: inverted index (identify, its category) | 46.7 KB | 1418 | 98.6% | 100.0% | 100.0% |
| 1200 | graph: features → category (categorise only) | 16.0 KB | 16 |  | 99.4% | 100.0% |
| 1200 | bit vectors 256: nearest signature (identify) | 38.7 KB | 7551 | 93.4% | 99.8% | 100.0% |
| 1200 | bit vectors 256: nearest prototype (categorise only) | 256 B | 2696 |  | 90.6% | 97.8% |
| 1200 | bit vectors 1024: nearest signature (identify) | 151.2 KB | 24130 | 98.2% | 99.8% | 100.0% |
| 1200 | bit vectors 1024: nearest prototype (categorise only) | 1024 B | 10403 |  | 97.8% | 99.6% |
| 12000 | graph: inverted index (identify, its category) | 431.2 KB | 12127 | 97.2% | 99.6% | 100.0% |
| 12000 | graph: features → category (categorise only) | 16.0 KB | 19 |  | 98.0% | 99.6% |
| 12000 | bit vectors 256: nearest signature (identify) | 386.7 KB | 52063 | 84.4% | 97.6% | 99.6% |
| 12000 | bit vectors 256: nearest prototype (categorise only) | 256 B | 2621 |  | 90.2% | 96.8% |
| 12000 | bit vectors 1024: nearest signature (identify) | 1511.7 KB | 190956 | 95.8% | 99.4% | 100.0% |
| 12000 | bit vectors 1024: nearest prototype (categorise only) | 1024 B | 10352 |  | 96.2% | 99.4% |
| 120000 | graph: inverted index (identify, its category) | 4271.2 KB | 132094 | 92.4% | 99.6% | 99.6% |
| 120000 | graph: features → category (categorise only) | 16.0 KB | 25 |  | 99.0% | 99.8% |
| 120000 | bit vectors 256: nearest signature (identify) | 3867.2 KB | 650675 | 73.8% | 98.8% | 99.6% |
| 120000 | bit vectors 256: nearest prototype (categorise only) | 256 B | 2740 |  | 90.0% | 97.6% |
| 120000 | bit vectors 1024: nearest signature (identify) | 14.8 MB | 2582530 | 88.8% | 99.4% | 99.6% |
| 120000 | bit vectors 1024: nearest prototype (categorise only) | 1024 B | 10987 |  | 97.8% | 99.2% |

## Bit vectors where they might win: episode recall

*Exact*: the recalled episode has the best activation-weighted overlap with the probe (ties count). *Quality*: the recalled episode's overlap as a share of the best.

| Mind | Episodes | Method | Bytes per episode | ns per query | Exact | Quality |
|---|---|---|---|---|---|---|
| 1k | 1500 | list: 8 × (u32 id, u16 activation) | 48 | 34894 | 100.0% | 100.0% |
| 1k | 1500 | list: 8 × (u32 id, u8 activation) | 40 | 35483 | 51.6% | 99.8% |
| 1k | 1500 | bit vectors 128 | 16 | 11330 | 29.0% | 89.4% |
| 1k | 1500 | bit vectors 256 | 32 | 14155 | 35.0% | 92.5% |
| 1k | 1500 | bit vectors 1024 | 128 | 31408 | 39.3% | 95.9% |
| 10k | 1500 | list: 8 × (u32 id, u16 activation) | 48 | 22006 | 100.0% | 100.0% |
| 10k | 1500 | list: 8 × (u32 id, u8 activation) | 40 | 21889 | 57.0% | 99.7% |
| 10k | 1500 | bit vectors 128 | 16 | 6677 | 31.0% | 90.3% |
| 10k | 1500 | bit vectors 256 | 32 | 8635 | 35.6% | 94.1% |
| 10k | 1500 | bit vectors 1024 | 128 | 26804 | 39.0% | 96.8% |
| 100k | 1500 | list: 8 × (u32 id, u16 activation) | 48 | 31661 | 100.0% | 100.0% |
| 100k | 1500 | list: 8 × (u32 id, u8 activation) | 40 | 32255 | 58.6% | 99.7% |
| 100k | 1500 | bit vectors 128 | 16 | 6619 | 36.0% | 87.8% |
| 100k | 1500 | bit vectors 256 | 32 | 8557 | 40.3% | 92.3% |
| 100k | 1500 | bit vectors 1024 | 128 | 26889 | 44.3% | 94.5% |
| 1M | 1500 | list: 8 × (u32 id, u16 activation) | 48 | 27941 | 100.0% | 100.0% |
| 1M | 1500 | list: 8 × (u32 id, u8 activation) | 40 | 28008 | 56.6% | 99.8% |
| 1M | 1500 | bit vectors 128 | 16 | 6876 | 24.3% | 89.1% |
| 1M | 1500 | bit vectors 256 | 32 | 9010 | 31.3% | 93.5% |
| 1M | 1500 | bit vectors 1024 | 128 | 37869 | 35.3% | 96.0% |
