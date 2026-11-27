# Spike: the mind lab's shared mind (stages 1 and 2)

One mind library for every mind-lab scenario: teaching, danger, the hunt and the village build on it. It is the fox's mind from `spikes/thought-graph`, extracted and made general (any species, any number of minds), on the compact runtime form the compaction lab (`spikes/mind-compaction`) recommended. Headless, deterministic, integer maths only, no dependencies. The original fox is a dev-dependency, used only to prove the port faithful.

```bash
cargo test --release -- --nocapture --test-threads=1    # 45 tests, ~50 s; each prints its numbers
cargo test --release --test storage -- --nocapture       # bytes and µs at fox size, 10k and 100k nodes
cargo test --release --test sleep_keep -- --nocapture    # the brief-09 question: sleep keeps 1/8 or 1/3
cargo test --release --test sensitivity -- --nocapture   # each outcome over 4 seeds × 5 temperaments
./tools/wasm-hashes.sh                                    # native vs wasm32 (under node) hashes
cargo run --release --example viewers && NODE_PATH=$(npm root -g) node tools/frames.cjs
```

## What's in it

| Module | What |
|---|---|
| `graph` | Link types, node tags, anchors, the founding-graph builder. Every node has a **stable key** (a hash of its origin); a link is addressed by a `LinkKey` (source, position). |
| `think` | Bounded, modulated spreading. **Fixed-point gain and flow are the reference.** **Ties break by stable key**, never by index. **Swap-in state**: dense activation is per-thread scratch; a mind keeps only its parked active list. The old arithmetic and index ties remain as ablations (`Params::exact`, `Params::index_ties`). |
| `store` | Two forms of one mind behind the `Store` trait. **`Runtime`**: a shared species `Base` (gap-coded varint targets, one byte type + valence flag, one byte weight in steps of 4, link valence only where present), a per-mind `Overlay` (dense node valence, own anchors, sparse reweights and link valences, added nodes and links), and sparse fast weights. **`Expanded`**: plain CSR, every value spelled out, for the viewer, tests and debugging. Lossless conversion both ways. |
| `mind` | The mind: senses and prediction error, absence surprise, modulators (hunger, arousal, mood, fatigue, stress), workspace, Hebbian fast weights, extinction, episodes, the backlog as sleep pressure, idle consolidation, naps and sleep, learnt places, volatility, puzzles and search, novelty, temperament. Generic over the form and over a `Species` (its concepts, the kinds of thing it can tell apart, its founding graph). Reads a `Sense`, returns an `Action`. |
| `fox` | The first species: the valley (now able to hold many foxes), the founding, the scenarios, `Sim` (one fox, recorded) and `Pack` (many foxes sharing one valley and one base, stepped on any number of threads in any order), the home experiment, the viewer. |
| `scale` | Big minds: the fox's founding graph plus a generated body of knowledge (10k, 100k nodes), so the real scenarios run at scale. |
| `people` | Stage 2, teaching. People as a species (concepts in the shared base; places, landmarks, the others and trust on each person's own overlay), a valley with sound in the air and paper notes, hearing and grounding a described place in one's own landmarks, source links, checking a claim against the world, mistake-or-lie evidence, notes and maps, talk chosen from the mind's own state, the fox shown the spring (`fox_demo`), the scenario runs (`runs`) and a viewer. |
| `hashes` | The pinned determinism runs, also built for wasm32 (`src/bin/hashes.rs`). |

**The open design question is a setting:** `Config::sleep_keep` (default 8): sleep keeps 1/`sleep_keep` of the day's plain strengthening. `tests/sleep_keep.rs` reports 1/8 against 1/3.

Ablation: `Config::overlay_steps_of_4` stores the mind's own learnt weights in steps of 4, as the base does.

## Tests

| File | What it shows |
|---|---|
| `faithful.rs` | With the three new choices off, the shared mind reproduces the original fox **tick for tick** (80 runs over 4 seeds × 5 temperaments, 120,400 ticks, plus 11 named scenarios). Then each choice is switched on alone, and decisions are compared. |
| `outcomes.rs` | Decision 08's outcomes 1–4 and 7, and temperament, on the shared mind. |
| `sleep_pressure.rs`, `home_moves.rs` | Stage 0's sleep-pressure and home-move tests, on the shared mind. |
| `forms.rs` | Runtime and expanded forms agree on every tick (6,802 ticks); conversions (and recompiling) are lossless; out-of-range link keys are refused; learning operations, added nodes (a told fact) and consolidation agree; renumbering a mind changes nothing with key ties; 100 foxes share one base. |
| `determinism.rs` | Pinned hashes; 24 foxes in one valley on 1, 2, 4 and 8 threads, minds in order, reversed and shuffled: one hash; the hash reacts to a changed seed, temperament, setting or a single fast weight; native and wasm32 builds agree. |
| `storage.rs` | Bytes per mind (base, overlay, fast, state) and µs per tick, think, consolidation, sleep and told fact, at 27, 10k and 100k nodes, runtime against expanded. |
| `sleep_keep.rs`, `sensitivity.rs` | Reports, not pass marks: the brief-09 question, and how far each outcome generalises. |
| `teaching.rs` | Stage 2's nine scenarios over 6 seeds × 3 temperament sets, each printing its numbers. |
| `teaching_determinism.rs` | Pinned teaching hashes; 4 people on 1, 2, 4, 8 threads in 3 orders: one hash; the hash reacts to small changes. |
| `teaching_storage.rs` | What a told fact, source link, note and map cost; source links over a year; µs per person-tick. |

Findings, numbers and what changed: `research/mind-lab/01-shared-mind.md` in the private store.

Teaching viewer pages: `cargo run --release --example teaching_viewers && NODE_PATH=$(npm root -g) node tools/frames.cjs out/teaching viewer/teaching-frames.json`. Findings: `research/mind-lab/02-teaching.md` in the store.

## Stage 2 changes to the shared library

- Day learning rounds stochastically (`Config::stochastic_learning`): calm, dim co-activation now learns. It moves the fox's pinned hashes and makes a sleepless fox's backlog level off on day 4.
- Forgetting of a mind's own learnt links (`Config::own_fade`, per link type, pruned below `PRUNE_BELOW`); a `source` link type; `Store::remove_link`.
- Places can grow (`Mind::add_place`, `link_near`; a species with no spare slots grows them), with a resize of the thread's activation scratch mid-step.
- `Sense::cues` (heard or read, already grounded), `KindInfo::tracked` (a moving thing seen near where it was last seen is predicted).

## Notes for later stages

- A mind is `Mind<Runtime>` built from a `Species` (`Mind::new`). New species supply a `Base`, `Roles`, a kinds table and geometry. People, utterances and notes aren't here yet. The hooks are `Store::add_node` / `add_link` (told facts, new places, objects as anchored nodes, all with the mind's own stable keys) and `Mind::imagine` (cued thought without changing the mind).
- **Home pulls too hard after a quiet day** (stage 2): with calm learning no longer truncated, home ↔ rest and home ↔ comfort saturate in a day at home, and comfort isn't gated by need, so a thirsty person can stay home. Open for brief 09.
- `Mind::backlog()` walks the held episodes' links, and a tick calls it about 5 times. At 10k–100k nodes that is most of a tick beyond the think. Cache it if it matters.
- Node keys are 32-bit: at 100k nodes 2 collide, and those ties fall back to the index.
- `Runtime::recompile` folds a mind's overlay into a base of its own, keeping every link in place (weights to steps of 4).
- Night still teleports the fox to its believed home; people sleep where they are and walk home at dusk if it pulls.
