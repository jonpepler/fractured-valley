# Spike: a fox's mind as a modulated thought graph

A fox in a small grounded world, with a mind that is one slow associative graph. Thought spreads through the graph each tick. Hunger, arousal, mood, fatigue and stress change *how* it spreads; they are not nodes. Fast weights on the links hold the recent past as a **backlog** that idle think budget and sleep consolidate, and the size of that backlog *is* the fox's tiredness. A compiled bit-vector form runs the same think. Headless, deterministic, integer maths only, no dependencies. This is a base for later tests, not the engine. It supersedes the memory model of `spikes/brains-talk`; the hash mixer and state hasher are copied from there.

```bash
cargo test --release -- --nocapture --test-threads=1   # 23 tests: outcomes (12), sleep pressure (5), home moves (6)
cargo run --release --example home_days 8               # the home experiment, day by day, every variant
cargo run --release --example home near 10              # one variant tick by tick (near|mid|far|removed|second|daily)
cargo run --release --example pressure                  # backlog, fatigue and consolidation over a few days
cargo run --release --example viewers                   # writes viewer/*.html (self-contained pages)
NODE_PATH=$(npm root -g) node tools/frames.cjs          # PNG frames of key moments → out/frames/
cargo run --release --example bench                     # 1k / 10k / 100k nodes
cargo run --release --example sweep                     # first choice by hunger × arousal
```

Open any `viewer/*.html` in a browser. Play animates each tick hop by hop; `?t=326&h=0` opens a moment.

## The world (`world.rs`)

A 64 × 40 grid of 1 m cells. Six places: den (home), stream, henhouse (the dog guards it), berry bush (farther, poorer food), camp (a campfire always burns there) and meadow. The den holds a **burrow**, a thing the fox sees like any other. The fox has hunger and pain (the world's `fatigue` counter is no longer read by the mind: tiredness comes from the backlog). It walks 1 cell a tick and flees at 2, or 3 when stressed. It sees things within 10 cells; there's no line of sight and no smell. The dog notices the fox within 8 cells, chases at 3 cells a tick and bites (pain 600). Scripts can set a fire, make the dog chase, remove the dog, set hunger, bring night, and move, remove or duplicate the burrow. Night puts the fox where its *mind* believes home is (the place that feels best to a tired, calm fox), not where the world's den is.

## The mind

- **Packed graph** (`graph.rs`): CSR arrays for offsets, targets, slow weight, link valence and link type, plus node valence and tags. There are no node or link objects.
  - Anchored nodes carry a world reference: places with real coordinates, and things or creatures by entity id with where they were last seen.
  - Everything else is an open connector: hunger, food, danger, comfort, anger…
- **Founding graph built from the world** (`fox.rs`):
  - concept links are authored;
  - near links come from real distance between every pair of places (weight `600·(48−d)/48`);
  - like links join places holding the same kind of thing;
  - holds and at links join each place to the things within 6 cells of it;
  - every place and thing gets empty `feels` slots (weight 0), so experience can write there without repacking.
- **Think** (`spread.rs`): bounded spreading, generic over the mind's form.
  - 4 hops, a frontier of 48, a decay of 550‰ per hop, ties broken by node id.
  - A node splits what it sends across its links (fan-out), and total activation is capped by an attention budget (6000).
  - Last tick's thought carries over at 500‰, and only 300‰ of it spreads again, so a thought nothing feeds fades.
  - The workspace is the top 8.
- **Modulators** scale how activation crosses each link (`spread::gain`):
  - hunger strengthens food-ward links, fatigue rest-ward ones;
  - arousal and low mood strengthen links into negative things;
  - arousal and stress damp idle place-to-place drift;
  - stress strengthens links into anger.
  - Mood is a slow average (1/32 per tick) of the workspace's valence.
- **Prediction** (`mind.rs`): each place holds learnt expectations for each kind of thing, and whether it's approaching.
  - A seen thing that matches is ignored: it gets 40 activation.
  - A prediction error injects the thing and its place, raises arousal, and damps the carried-over thought (an interrupt).
- **Action** reads the workspace:
  - **flee** if danger is on the mind above a bar (lower when stressed) and its sensed source is in the workspace;
  - **snap** if the dog is within 2 cells and anger is high enough against danger (the bar falls with aggression);
  - **eat** food within reach;
  - otherwise **walk to** the place a workspace node points at, scored by *activation × how the place feels*. The feeling is a one-hop look-ahead over its links, scaled by the drives and modulators (a somatic marker). A place that feels bad is rejected and pushed down, so alternatives come up.
- **Day learning:** links between co-active workspace nodes gain fast weight, scaled by arousal; fast weights fade by 1/128 a tick. Salient moments (an arousal jump or pain) go into an episode buffer.
- **Consolidation and sleep pressure** (owner, 27 Nov; replaces the old sleep model):
  - **Backlog** = fast weight (by size; extinction leaves negative traces) + what each unreplayed episode would still write. Fast weights no longer vanish at night: they fade very slowly (1/4096 a tick) and leave by consolidation, or by lossy compaction over capacity (30 000: every trace scaled to 90%, and that work is taken from the think).
  - **Waking always encodes:** co-active links gain `co × (arousal + 60) / 4000` per tick, soft-bounded on the trace, so well-known links still collect potentiation (synaptic homeostasis, Tononi & Cirelli). Links touching a place learn only while the fox is there.
  - **Idle time is consolidation time:** each tick has a budget of 600 link visits. On a settled tick (no surprise, no pain, not fleeing, stress < 400), whatever the think didn't use goes to the backlog, whatever the body is doing: first replaying the most aroused episode (2 hops over its links; a quarter of its strength per replay awake), then fast weights, largest first, at 60 units of budget per unit of weight. Plain fast weight keeps 1/8 in the slow graph (the rest is downscaled away); replayed episodes keep `strength/6` per link, plus their feeling on anchored places and things.
  - **Sleep** is the same work with a big budget: 8000 a tick, 240 ticks a night, full-strength replay. A tired fox (fatigue ≥ 500) at a place where it *sees* a burrow, calm (arousal < 300, stress < 400), with rest on its mind, **naps**; a surprise, pain, hunger ≥ 900 or a cleared backlog wakes it.
  - **Tiredness is the backlog:** `mods.fatigue = backlog × 1000 / capacity`. It injects fatigue (→ rest → home), damps all spreading by up to a third, makes stress rise faster (×(1 + fatigue/2000)), raises anger's gain and lowers the snap bar (irritability).
  - `skip_sleep` now means a fox that can't sleep: at night its mind runs 240 waking ticks lying where it is.
- **Home, places and surprise without special cases** (for the home experiment; all general):
  - **Recognition by look and place, not identity.** A known fixed thing is the visible thing of its kind within 6 cells of where it was; a creature, the nearest of its kind. Anything else is just its kind ("a burrow" = rest). "Where am I" and "which place is this thing at" use the mind's own map, never the world's labels.
  - **Learnt places.** Seeing a landmark (burrow, food, water) where no place is known takes one of 4 spare place slots (when full, the weakest learnt place is forgotten and reused) and links it by real distance to the rest.
  - **Absence surprise.** An expected landmark missing from a place within 5 cells is a prediction error: the missing thing and its place come to mind, `loss` (frustration, val −500, → anger) if it was wanted, and arousal and stress rise as for any surprise.
  - **Extinction** (Rescorla–Wagner): the missing thing's links to its place, and to what it stood for (burrow → rest), get a negative fast trace; `loss` attaches to the place by prediction error only, never by co-activation. A link under extinction isn't re-strengthened by Hebbian learning, and a replayed episode consolidates its extinction.
  - **Search** (curiosity from unresolved prediction error): an absence leaves a puzzle; when the best option is the very place the thing went missing from, or nothing pulls, the fox walks seeded waypoints in widening rings around that place (4 → 30 cells). Finding that kind of thing elsewhere, or time, resolves it.
  - **Intrigue:** a surprising thing that isn't bad adds `error × 0.6` to its place's score (curiosity is about the unknown, not value), and an unfamiliar thing that's wanted pulls toward where it's seen.
  - **Volatility** (Yu & Dayan 2005; Behrens et al. 2007): per kind, how often predictions about where it is fail. It down-weights those errors' arousal (up to 70%) and speeds up the search drive. "Home is somewhere": expectation at the level of a kind.
  - **Flight** needs a bad thing (negative valence) to flee from; anger alone no longer counts as a source.
- **Stress and temperament** (owner addition):
  - Stress (0..1000) rises with pain, surprise and danger on the mind, and decays 1/40 a tick.
  - It cuts thinking: at full stress a very reactive fox thinks 1 hop through a frontier of ~6, against 4 hops through 48.
  - It lowers the flee bar, makes flight faster and favours anger.
  - Each individual's `Temperament` (reactivity, aggression) is seeded from `hash(seed, individual)`. It is expressed as structure and parameters: how fast stress rises, how much it cuts thinking, the danger→anger and pain→anger link weights, and the snap bar.

## Compiled forms

- **Bit vectors** (`hd.rs`): each node's out-links compile into one 2048-bit bundle.
  - The bundle is a majority vote over the targets' random item vectors, repeated 1–4× by weight.
  - Thinking recovers links by **cleanup**: popcount similarity against every item vector, with the weight decoded from how far above chance it is.
  - Optional **XOR binding** keeps link types: `item ⊕ role(type)`, probed once per type the node uses.
  - Link valence is lost. `decompile` expands the form back into a CSR graph for the viewer.
- **Packed** (`packed.rs`): the graph quantised (one byte for weight, one for type, one for valence). A second data point for decision 8.6.

## Results (see the tests' printed output)

### Outcomes (`tests/outcomes.rs`)

| Outcome | Result |
|---|---|
| 1. Modulation | Same slow graph, 4 states, 3 different choices: neutral and hungry go to the henhouse, afraid stays, low mood goes to the berry bush. Activation differs by 54–426‰ between states and every chain of thought differs. Fear doubles the dog's activation (19→38) and drops the henhouse's feeling (350→98). **Partial:** low mood does not bring the dog to mind (0); it dims good paths instead, the hens more than the berries. |
| 2. Surprise | The campfire is seen on 16 ticks (largest error 57) and ignored. The meadow fire (error 1000) gives arousal 0→460, danger 712, and the fox flees **in the same tick**. A fox that expects fire in the meadow walks on. |
| 3. Places | Den→henhouse is 26 cells (near weight 275); den→berry bush is 49 cells (0); henhouse ~ berry bush like weight 300. Now compared with day learning off (as outcome 1): at arousal 400 the fox picks the berry bush at hunger 700–900 and the henhouse at 1000; at arousal 600 never the henhouse. At the den, mood goes −500→+242 in 80 ticks, against −44 in the meadow and −4 by the stream. |
| 4. Sleep | Updated. One bite. Henhouse→danger 0→225 and →pain 0→149 in the slow graph **by nightfall** (idle time and a nap at the den), unchanged by the night; fast weights 22 839→0 overnight. Next morning at hunger 700 the bitten fox picks the berry bush, evoking 109 danger (never met the dog: henhouse, 60). The henhouse now feels +262 to it, against +717: hunger still outweighs the fear (it was −512). **Changed:** a sleepless fox keeps the fear (114) and stays home. |
| 5. Compile | **Now 215/300 (72%)** for the same action, 258/300 for the top thought: the learnt graph is denser and the bundle inflates weak links (den→danger 82 decodes as ~260). Before: 2048 bits: the same action on 249/300 sampled states (83%) and the same top thought on 241/300. Typed binding: 264/300. 1024 bits: 274/300. More bits don't help: the misses are structural (lost link valence, merged types), not noise. Decompiled: 89/92 links, 0 spurious, mean weight error 108/1000. **Not smaller or faster:** 5678 B against 2065 B, and 7× slower per think. The packed form is 690 B and as fast. |
| 6. Scale | Graph think: 25 / 27 / 31 µs at 1k / 10k / 100k nodes, at 135 B per node. Bit vectors: 2.7 ms / 39 ms / 357 ms at 265 B per node (+256 B codebook). Packed: 77 B per node, 24–28 µs. |
| 7. Viewer | 15 self-contained pages in `viewer/` (new: `sleepless`, `home-near`, `home-mid`, `home-removed`, `home-second`; `dog-and-sleep` now shows the nap), with PNG frames from `tools/frames.cjs`. |
| 8. Determinism | Pinned run hash `0x88087aeeb83a2838` (was `0xe8a42f9338b431fb`; behaviour changed by design) (two days, a night and a fire), and identical across runs. The compiled form is identical too. Foxes stepped on threads in reverse order match a sequential run, and two minds over one world view match in either stepping order. |
| Stress | Calm fox #26 (reactivity 1, aggression 185): 4.0 hops, 206 links per think, full-speed flight after 8 ticks, never snaps. Quick fox #18 (808, 862): 2.1 hops, 92 links, full-speed flight after 1 tick, snaps twice when cornered. Both react in the first tick. |

### Sleep pressure (`tests/sleep_pressure.rs`)

| Test | Result |
|---|---|
| (a) Skipping sleep | A bitten fox kept awake has henhouse→danger 204 in the slow graph by nightfall (239 settled ticks, 19 replays) and next morning thinking of the henhouse brings danger 114 (never met the dog: 60). It doesn't go to the henhouse. With idle consolidation off, nothing reaches the slow graph (0) and the fear sits only in fast weights. |
| (b) Fills up | Quiet sleepless days: backlog at nightfall 7 813 → 9 512 → 13 454 → 13 894, fatigue 260 → 463. Eventful sleepless days: 24 215 (fatigue 806) on day 1, ~21 000–24 000 after. A tired think is **narrower, dimmer and less faithful, not costlier**: 123 links visited, against 131 for its own slow graph rested; recall overlap with the slow graph 875–916‰ (rested 1000‰); activation reaching the workspace 2 555–2 605 against 2 918. Decisions: rested goes to the henhouse at hunger 700, tired stays home (and a kept-awake fox sits at its den even at hunger 1000). Charged by the dog: snaps 2 against 1, first reaction a tick later. |
| (c) One night | Backlog 22 155 → 0, fatigue 738 → 0, recall overlap 916 → 1000‰, mass 2 588 → 2 841. The slow graph itself grew: a rested think now visits 145 links, not 94. |
| (d) Busy vs quiet | Quiet day: backlog 7 816 at nightfall, peak fatigue 261, no naps. Busy day (bite, fire, a second chase): peak fatigue 910, 21 ticks napping, backlog 22 839; without naps 24 226. |
| (e) Determinism | Sleepless, busy and home runs give identical hashes twice and on threads. Pinned outcome hash is now `0x88087aeeb83a2838`. |
| Compute | Idle consolidation ran on 449 of 451 waking ticks, spending 400 units (the think used ~170 of 600): ~457 ns, against ~3 230 ns for a think. |

Compaction never triggered in these runs (lost 0): the backlog peaked at ~24 000 of 30 000.

### Home moves (`tests/home_moves.rs`, `examples/home_days.rs`)

On day 1 at tick 100, while the fox is at the henhouse, its burrow is moved, removed or doubled. No dog. Nothing here is special-cased.

| Variant | What the fox experiences |
|---|---|
| Near (9 cells, in sight) | Wakes at the old den: absence error 918, a novel burrow 1000, arousal 900, stress 974, loss 729. The workspace is den, the burrow, rest, loss. **Intrigue:** it walks to the new burrow in 24 ticks and naps there. It sleeps at the new burrow on night 3, then **alternates** for 12 nights: each night at the new burrow, sleep downscales the day's unconsolidated extinction, so the old den wins the next night (spontaneous recovery). The shock of each return fades: absence 918 → 417, early arousal 896 → 93. |
| Mid (18 cells, out of sight) | Same shock, then **search** (158 and 153 ticks on days 2–3) in rings around the old den, with repeated shocks as it passes the empty den (a sawtooth in arousal). It doesn't find the burrow by searching; on day 4 it sees it from the stream, goes to it in 6 ticks and sleeps there that night. Then home drifts to the stream. |
| Far (46 cells) / Removed | Identical inside the fox: search (614 ticks over 8 days), loss and some anger, mood down to 30. Never found. The old den sours (felt 1005 → −227) and the stream becomes where it sleeps from night 3. |
| Second, identical burrow | A burrow where none was: error 1000, arousal 900, stress 974 for ~50 ticks, no loss, no search. It stays near the den (297 ticks) and never visits; the old den stays home. |
| Moved every night (14 days) | Learnt volatility of burrows climbs to 900. 10 of 10 later absence shocks start calmer than the first (early arousal 896 on day 2; 123–330 on many later days), but not monotonically: a new place, or a day after volatility has decayed, shocks it fully again (day 8: 846). Home wanders between the old den, learnt places and the stream. |

**Search, intrigue, confusion or panic?**
- **Panic: no.** No flight in any variant (flight needs a bad thing to flee). But it is *alarmed*: arousal and stress hit their ceilings for 20–160 ticks.
- **Search: yes**, whenever the burrow is missing and nothing nearby replaces it (Mid, Far, Removed); it comes from the unresolved prediction error, ringing the place it went missing.
- **Intrigue: yes**, when a burrow is seen where none was and nothing bad is attached (Near, Mid, Daily); it goes to look within 6–24 ticks. Not for the second burrow while its own is still there: the den outscores it.
- **Confusion: partly.** Workspace turnover after the shock is 4–91‰ a tick against 25‰ on an ordinary morning. Mostly it's lower: the fox perseverates on den, burrow, rest and loss. It runs higher on later days of Far/Removed, when the search keeps failing.

## What makes the charging dog a surprise, and what makes it run (owner's question)

- **The surprise** is a prediction error, not the dog itself: at each place the fox expects each kind of thing, approaching or not. The dog at the henhouse, *standing*, is expected (900) and ignored. The dog *approaching* has never been seen there (expectation 0), so the error is 1000. That injects the dog's node (1000) and its place (500), raises arousal by 500, damps the carried-over thought by 70%, and raises stress.
- **The impulse to run**: the dog node's founding link to danger (dog ↔ danger, `kind` 700) carries that injection to `danger` within a hop. `choose` then flees if danger is in the workspace above a bar (300, lowered by stress) *and* a sensed bad thing in the workspace is its source. That is a threshold rule, not a learnt policy.
- **The running** is grid steps: 2 cells a tick, 3 when stress ≥ 500, directly away. There is no body, energy, gait or momentum; "running faster" is just the step count.

## What's faked or missing

- **Night teleports the fox** to where its mind believes home is. It doesn't walk there, and it can sleep at a place with no burrow.
- **The new mechanisms are tuned too.** Budgets, costs, the kept share (1/8), the fade, capacity and the extinction rate were set while building, against these tests. Re-anchoring is sensitive to the kept share: at 1/3 the fox settled at the new burrow within 5 nights, but then the routine consolidated so much that a bitten fox's henhouse felt good again; at 1/8 the fear holds but home alternates.
- **No smarter search:** waypoints ring the loss point; it never thinks "burrows are by the stream". It found the mid burrow on its rounds, not by searching.
- **Sleep-pressure effects are partly modulator rules:** narrower spreading, faster stress and irritability are written into how `fatigue` acts. What's emergent is that fatigue *is* the backlog, so experience sets it.
- **A tired think isn't more expensive** (it visits fewer links). Lossy compaction never ran in these scenarios.
- **Recognition is crude:** a fixed thing is "the same" only within 6 cells of where it was.
- **No absence surprise.** An empty berry bush isn't noticed as missing; berries regrow each night.
- **Founding knowledge is hand-set.** Valences, link weights and constants are hand-tuned. The 8 outcomes were tuned against while building, so they show the mechanisms can produce the behaviour, not that they do it robustly. The sweeps show how narrow some margins are.
- **One fox.** The thread and brain-order test uses separate worlds, or two minds over one world view; the world doesn't hold two foxes.
- **Arousal also speeds up day learning**, so a held fear strengthens whatever is on the mind, including the henhouse→hens link. Outcome 1 turns day learning off to compare modulators alone.
- **Valence lands roughly.** Sleep moved the stream's valence too: the fox fled past it.
- **The compiled form is a snapshot.** It is recompiled every 10 ticks in the compiled run, and day learning happens in the graph form.
- **Thinking with the compiled form scans every node**, so it is O(N) per probe. That's fine for a fox and useless for a person-sized mind on a CPU.
- **Sight only:** no smell or sound.
