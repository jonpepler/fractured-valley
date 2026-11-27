# Spike: a fox's mind as a modulated thought graph

A fox in a small grounded world, with a mind that is one slow associative graph. Thought spreads through the graph each tick. Hunger, arousal, mood, fatigue and stress change *how* it spreads; they are not nodes. Fast weights on the links hold today, sleep consolidates them, and a compiled bit-vector form runs the same think. Headless, deterministic, integer maths only, no dependencies. This is a base for later tests, not the engine. It supersedes the memory model of `spikes/brains-talk`; the hash mixer and state hasher are copied from there.

```bash
cargo test --release -- --nocapture --test-threads=1   # 12 tests: the 8 outcomes + stress/temperament
cargo run --release --example viewers                   # writes viewer/*.html (self-contained pages)
NODE_PATH=$(npm root -g) node tools/frames.cjs          # PNG frames of key moments → out/frames/
cargo run --release --example bench                     # 1k / 10k / 100k nodes
cargo run --release --example sweep                     # first choice by hunger × arousal
```

Open any `viewer/*.html` in a browser. Play animates each tick hop by hop; `?t=326&h=0` opens a moment.

## The world (`world.rs`)

A 64 × 40 grid of 1 m cells. Six places: den (home), stream, henhouse (the dog guards it), berry bush (farther, poorer food), camp (a campfire always burns there) and meadow. The fox has hunger, fatigue and pain. It walks 1 cell a tick and flees at 2, or 3 when stressed. It sees things within 10 cells; there's no line of sight and no smell. The dog notices the fox within 8 cells, chases at 3 cells a tick and bites (pain 600). Scripts can set a fire, make the dog chase, remove the dog, set hunger, or bring night.

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
- **Sleep:** episodes are replayed into fast weights. Fast weights are consolidated into slow ones (a third of their weight) and then cleared. Each episode's feeling moves the valence of the anchored places and things in it, never the concepts.
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

| Outcome | Result |
|---|---|
| 1. Modulation | Same slow graph, 4 states, 3 different choices: neutral and hungry go to the henhouse, afraid stays, low mood goes to the berry bush. Activation differs by 54–426‰ between states and every chain of thought differs. Fear doubles the dog's activation (19→38) and drops the henhouse's feeling (350→98). **Partial:** low mood does not bring the dog to mind (0); it dims good paths instead, the hens more than the berries. |
| 2. Surprise | The campfire is seen on 16 ticks (largest error 57) and ignored. The meadow fire (error 1000) gives arousal 0→460, danger 712, and the fox flees **in the same tick**. A fox that expects fire in the meadow walks on. |
| 3. Places | Den→henhouse is 26 cells (near weight 275); den→berry bush is 49 cells (0); henhouse ~ berry bush like weight 300. At arousal 400 the fox picks the berry bush at hunger 800 and the henhouse at 900; at arousal 600 never the henhouse. At the den, mood goes −500→+242 in 80 ticks, against −44 in the meadow and −4 by the stream. |
| 4. Sleep | One bite. Henhouse→danger goes 0→226 and henhouse→pain 0→150 (slow weights); henhouse valence 0→−36; fast weights 1285→0. Next morning at hunger 700: the bitten fox picks the berry bush, the henhouse feels −512 and evokes 109 danger. A fox that never met the dog picks the henhouse (+545, 65). Without sleep, the fear is gone by morning (henhouse). |
| 5. Compile | 2048 bits: the same action on 249/300 sampled states (83%) and the same top thought on 241/300. Typed binding: 264/300. 1024 bits: 274/300. More bits don't help: the misses are structural (lost link valence, merged types), not noise. Decompiled: 89/92 links, 0 spurious, mean weight error 108/1000. **Not smaller or faster:** 5678 B against 2065 B, and 7× slower per think. The packed form is 690 B and as fast. |
| 6. Scale | Graph think: 25 / 27 / 31 µs at 1k / 10k / 100k nodes, at 135 B per node. Bit vectors: 2.7 ms / 39 ms / 357 ms at 265 B per node (+256 B codebook). Packed: 77 B per node, 24–28 µs. |
| 7. Viewer | 12 self-contained pages in `viewer/`, with PNG frames from `tools/frames.cjs`. |
| 8. Determinism | Pinned run hash `0xe8a42f9338b431fb` (two days, a night and a fire), and identical across runs. The compiled form is identical too. Foxes stepped on threads in reverse order match a sequential run, and two minds over one world view match in either stepping order. |
| Stress | Calm fox #26 (reactivity 1, aggression 185): 4.0 hops, 206 links per think, full-speed flight after 8 ticks, never snaps. Quick fox #18 (808, 862): 2.1 hops, 92 links, full-speed flight after 1 tick, snaps twice when cornered. Both react in the first tick. |

## What's faked or missing

- **Night teleports the fox home.** It doesn't walk back.
- **No absence surprise.** An empty berry bush isn't noticed as missing; berries regrow each night.
- **Founding knowledge is hand-set.** Valences, link weights and constants are hand-tuned. The 8 outcomes were tuned against while building, so they show the mechanisms can produce the behaviour, not that they do it robustly. The sweeps show how narrow some margins are.
- **One fox.** The thread and brain-order test uses separate worlds, or two minds over one world view; the world doesn't hold two foxes.
- **Arousal also speeds up day learning**, so a held fear strengthens whatever is on the mind, including the henhouse→hens link. Outcome 1 turns day learning off to compare modulators alone.
- **Valence lands roughly.** Sleep moved the stream's valence too: the fox fled past it.
- **The compiled form is a snapshot.** It is recompiled every 10 ticks in the compiled run, and day learning happens in the graph form.
- **Thinking with the compiled form scans every node**, so it is O(N) per probe. That's fine for a fox and useless for a person-sized mind on a CPU.
- **Sight only:** no smell or sound.
