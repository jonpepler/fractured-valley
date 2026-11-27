# Spike: the mind lab's shared mind (stages 1 to 4)

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
| `danger` | Stage 3, danger with company. The wild **boar** as a species (`danger::boar`, built the fox's way, hunted so people anger it), and the scenarios (`danger::runs`): a hunter who expects a boar against a forager who doesn't, alone or with allies, a cry for help, and a plan under stress. |
| `hunt` | Stage 4, the hunt and the betrayal. A **deer** with an animal mind (`hunt::deer`); a hunting layer round a person's mind (`hunt::hunter`): a plan spoken as words (roles, places by landmarks, a whistle), grounded by each hearer in its own map, joined or not from its own feelings, held as an intent whose **predictions** of where the others will be break as prediction errors; and a plan against a person that one member, chosen by its own relationships, whispers to the target. Scenarios in `hunt::runs`. |
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

Stage 4: `cargo test --release --test hunt -- --nocapture` (the scenarios), `hunt_determinism.rs`, `hunt_storage.rs`; pages: `cargo run --release --example hunt_viewers && NODE_PATH=$(npm root -g) node tools/frames.cjs out/hunt viewer/hunt-frames.json`; quick sweeps: `--example hunt_sweep`, `--example betray_sweep`; `--example world_fingerprint` prints world-only hashes of the stage-2 and stage-3 pinned runs (behaviour checks when a change moves only the minds' hashes). Findings: `research/mind-lab/04-hunt.md` in the store.

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

## Stage 3 changes to the shared library

- **Choosing** (fixes from stage 2): comfort is gated by need (`Config::comfort_gate`: fatigue, stress or low mood), a place's worth is discounted by distance (`Config::walk_cost`, half at 120 cells), and a goal under way is kept unless beaten by `SWITCH_MARGIN` (`Config::hold_goal`). `tests/home_pull.rs` shows each alone and together. People also get evening drowsiness (`Body::drowsy`, the circadian half of the two-process model), so they still walk home at dusk.
- **Intent** (`Mind::form_intent`, `Held`): a plan node of the mind's own, rehearsed (topped up to `INTENT_HOLD`) while it is in the workspace, the rehearsal cut by stress as this temperament's stress cuts thought; dropped after `INTENT_LAPSE` ticks out of mind. A held intent **predicts** what it links to (smaller prediction error) and **focuses** spreading (`Mods::focus`: flows into the intent and its targets are multiplied by up to 2). `Config::focus` is the ablation.
- **Danger responses** (`Roles::resp`, `Mind::respond`): flee, fight, call, freeze, hold, and aid for hearers. Every response is a node; flee is the reflex (one hop from danger, in play once danger passes the flee bar, boosted by stress); the rest need more hops. Company is sensed (companions draw `COMPANY` × arousal attention), read through the mind's own learnt links to "someone who stands with me", and emboldens (`COURAGE`) and buffers stress (`BUFFER`). The voice is its own channel: a mind cries out while its call node is active enough. New actions: `Hold`, `Freeze`, `Call`, `Charge`. The fox has no responses and is unchanged.
- **People**: pain, a boar they can see, `Say::Help` (a shout heard by anyone in earshot, located by ear), a learnt ally link per person (`Founding::ally`, then a rule: seen standing with me while I'm in danger strengthens it, seen leaving weakens it), reactivity sets danger → freeze, and a habit of crying out (`Founding::calls`).
- **Fixed on the way**: a `Vec<usize>` hashed as raw bytes made native and wasm32 hashes part (`Person::owe_thanks`); the focus code read the thread's edge buffer uncleared (caught by the new order test). The wasm32 test now fails when it can't run (skip only with `FV_SKIP_WASM=1`).

```bash
cargo test --release --test danger -- --nocapture --test-threads=2   # the scenarios, per seed
cargo test --release --test danger_determinism -- --nocapture         # pinned, threads × orders
cargo test --release --test danger_storage -- --nocapture             # bytes and µs
cargo test --release --test home_pull -- --nocapture                  # the stage-2 fixes
cargo run --release --example danger_viewers && NODE_PATH=$(npm root -g) node tools/frames.cjs out/danger viewer/danger-frames.json
```

Findings: `research/mind-lab/03-danger.md` in the store.

## Stage 5: the village

Twenty to fifty people in one valley for a month: water, berry bushes, a deer, a foraging boar and a
few foxes, each with a mind of its own. Module `village` (`life`: the tier, gossip, the idea of a
hunt; `fox`; `runs`: the valley for a seed, the month, the measures).

```bash
cargo test --release --test village_determinism -- --nocapture   # pins; 1/2/4/8 threads × 3 orders; not blind
cargo run --release --example village_study -- 100 30 30 full,cheap,prune && python3 tools/village_report.py
cargo run --release --example village_month -- 30 30 1          # one valley, storage per night
cargo run --release --example village_threads -- 50 2           # the pool on 1, 2, 4, 8 threads: one hash
cargo run --release --example village_profile -- 30 2           # where a tick's time goes
cargo run --release --example village_surprise -- 30 1          # what surprises villagers, by kind
cargo run --release --example early_stop                         # the early stop on every pinned run
cargo run --release --bin village_bench -- 30 3                  # µs per mind-tick, native
cargo build --release --bin village_bench --target wasm32-wasip1 && node tools/wasi.mjs target/wasm32-wasip1/release/village_bench.wasm 30 3
cargo run --release --example village_viewers && NODE_PATH=$(npm root -g) node tools/frames.cjs out/village-frames viewer/village-frames.json
```

Library changes (each off unless a village turns it on, and hashed only when on, so every earlier
pin holds): people as a vector of any size (`people::world::{name, person_id, person_kind}`,
`species::village_people`); a second appetite (`Roles::hunger2`, `Mods::hunger2`); the voice model
(`world::voice_level`, `heard_voice`); `Config::{expect_at_edge, food_contingent,
felt_without_sources, habituate, prune_places}`; spent intents (`Mind::spent`); the cheap tier's
`Mind::upkeep`; `sim::Pool` (brains on persistent threads); the early stop in `think`
(`think::EARLY_STOP`, exact, on by default). Bit-identical speed-ups: `learn_expectations`, place
lookups once per perceive, the backlog read once per step. Findings: `research/mind-lab/05-village.md`
in the private store.

## Round 2, stage 1: foundations

```bash
cargo test --release --test round2 -- --nocapture                    # dormancy, feelings, gossip memory, sleep share, thinking rate
cargo run --release --example village_long -- 4 30 180 1 long-180    # a long run; per-day CSV in out/village/
python3 tools/round2_compare.py <round1 runs.csv> <round2 runs.csv>   # paired by seed, sign-tested
```

- **Needs only on spreading** (`Mind::felt_in`, `salience`): what answers a need counts by how strongly it (or its kind) is on the mind; a place lacking what an uppermost need wants is felt to lack it. `Config::needs_in_choice` is round 1's rule.
- **Courage on spreading** (`Mods::courage`, `Config::courage_spread`).
- **Thinking rate for every mind** (`Mind::every`, `phase`, `live`, `due`, `reflex`, `idle`; `Person::gate`): reflexes every tick, senses buffered, rates per tick (`dt`, `per_ticks`).
- **Feelings by evidence** (`people::person::Rel`, `feel_evidence`, `set_base`; beta reputation with forgetting), left alone by consolidation (`Config::protect_social`). Write trust and warmth only through these.
- **Gossip with memory** (`lt::TOLD`), sleep keeping more of a novel day (`Config::novelty_keep`), **dormancy** (`Mind::cold`, `go_dormant`, `revive`; `Config::dormancy`).
- The village's fixes are default; joining, warning and crying out are graded (`mind::graded`). `Config::round1()` restores round 1's rules for ablations.

Findings: `research/mind-lab/round-2/01-foundations.md` in the private store.
