# The thinking step, version 2: a spec

Round 2, stage 2 of the mind lab (decision 9.2, direction B). This is the definition of one think;
`src/think2.rs` is it as CPU code and `gpu/src/think2.wgsl` as a compute shader, and both give the
same bits (`gpu/tests`, `gpu-bench`, `village-check`). Sections 1–8 are the step; 9 and 10 are what
a tick does around it, including the look-ahead (`felt_in`) that choosing uses.

`Config::think_v2` selects it; `Config::round1()` keeps the old step (`think::think`, "v1").

## 1. Integer rules

- Every quantity is a 32-bit integer. `a / b` truncates towards zero (Rust's and WGSL's `/` on
  `i32` and `u32`). No floating point anywhere.
- `min`, `max` and `clamp(x, lo, hi)` are the usual ones.
- **Nothing wraps.** Every product is bounded below 2³¹ (signed) or 2³² (unsigned); §6 proves each
  bound. Where a value could exceed its range, it **saturates** at a stated cap and the think
  raises a flag (`SAT_*`), which is part of its output.
- **Ties** between nodes are broken by a total order: larger amount first, then smaller stable key
  (`Form::key`, a hash of the node's origin), then smaller node id. Ids only decide between two
  nodes whose 32-bit keys collide. Renumbering nodes changes nothing.

## 2. One think: inputs and outputs

**Inputs**
- The mind's graph (§7): for each node, its tags, stable key and valence, and its links with a
  positive effective weight.
- The state: a set of `(node, activation)` entries, each node at most once (the mind's active list
  as it was left, after anything the tick did to it, such as a damp).
- The modulators (`Mods`): hunger, hunger2, arousal, fatigue, stress (each 0..1000), mood
  (−1000..1000), focus with its intent node, courage with its ally node. Out-of-range values are
  clamped where they are read.
- The injection list: `(node, amount)` in order; a node may appear more than once.
- The params (`Params`), which must satisfy: `1 ≤ min_act ≤ max_act`; `persist`, `carry_spread` and
  `hop_decay` in 0..1000; `max_act ≤ A_CAP`; `budget ≤ 131,071`. The GPU form also needs
  `frontier ≤ 64`. Both forms assert these (the GPU's host before it dispatches).

**Outputs**
- The new state: `(node, activation)` sorted by node, at most `active_cap` entries, every
  activation in `min_act..max_act` (or positive when the budget did not apply, see 5.8).
- The workspace: the top `ws_k` of the new state by the tie order.
- `visits`: links of the senders expanded (§5.6), which sets the idle consolidation budget (§9).
- `hops_run`, and the saturation flags.

**Caps**

| Name | Value | Caps |
|---|---|---|
| `A_CAP` | 16,383 | a sender's amount; a stored or injected activation |
| `INC_CAP` | 2³⁰ | the sum of flows into one node in one hop |
| `F_CAP` | 262,143 | a link's factor (§4) |

## 3. Gain knots

A link's **gain class** is `type × 8 + tc`, where `type` is the link type (0..9) and `tc` the
target's tag class: bit 0 food, bit 1 rest, bit 2 anger (80 classes).

Per think, from the modulators: `a` arousal, `h` = max(hunger, hunger2), `f` fatigue, `s` stress,
each clamped to 0..1000; mood clamped to ±1000; `neg` = max(−mood, 0), `pos` = max(mood, 0);
`fk` = 1000 − f / 3; `hd` = hop decay.

For each class, with `x` starting at the type's base gain (`TYPE_GAIN`: 1000 for wants, kind,
holds, at, feels; 700 near; 600 like; 900 assoc, source, told):

1. Slow associative types (near, like, assoc, source, told):
   `x = x × (1000 − a/2) / 1000 × (1000 − s/2) / 1000`, left to right.
2. Anger-tagged target: `x = x + x × s × 2 / 1000 + x × f / 1000` (both terms from the same `x`).
3. Food-tagged: `x = x + x × h / 1000`. Rest-tagged: then `x = x + x × f / 1000`.
4. `g0 = max(x × fk / 1000, 0)`.
5. Three knots, at felt valence −1000, 0 and +1000, with multipliers
   `M− = 1000 + 2 × (a + neg)`, `M0 = 1000`, `M+ = 1000 + pos − neg`:
   `F_j = min(g0 × M_j / 1000 × hd / 1000, F_CAP)`.

`bt[type]` is the largest `F_j` over the type's 8 classes and 3 knots.

These are v1's gain (`think::Gain::fixed`, which is already linear in the valence on each side of
0) evaluated at its three knots, with the hop decay folded in: so the knots are v1's numbers, and
only the rounding between them differs.

## 4. A link's factor and a sender's bound

A link `u → t` of class `c`, link valence `lv`, into target `t` of valence `val(t)`:

- `v = clamp(val(t) + lv, −1000, 1000)`.
- **Interpolate:** `F = F0 + (F− − F0) × (−v) / 1000` if `v < 0`, else `F = F0 + (F+ − F0) × v / 1000`.
- **Focus and courage.** If focus > 0 and the intent is a node of the mind, the focus set is the
  intent and every target of its links, and `fg = 1000 + clamp(focus, 0, 1000)`; else `fg = 1000`.
  If courage > 0, the ally is a node of the mind and it has at least one link, the courage set is
  its links' targets and `cg = 1000 + clamp(courage, 0, 1000)`; else `cg = 1000`. Then
  `F = F × fg / 1000` if `t` is in the focus set, then `F = F × cg / 1000` if in the courage set.
- `F = min(F, F_CAP)` (flag `SAT_FACTOR` if it bound).

A sender's **bound** is `B = min(max_{type in u's links} bt[type] × fg / 1000 × cg / 1000, F_CAP)`.
Every link of `u` has `F ≤ B`: interpolation truncated towards zero never leaves the range of its
two knots, and the multipliers and the clamp are monotone.

## 5. The step

Scratch: activation `act` and hop sums `inc` per node, all zero outside a think.

1. **Persist.** For each state entry `(n, x)`: `x' = clamp(x, 0, max_act) × persist / 1000` (flag
   `SAT_INJECT` if `x > max_act`; a mind's own state never is, since every think leaves it ≤ `max_act`). If `x' ≥ min_act`, `act[n] = x'` and `n` is active; otherwise it
   is dropped.
2. **Carry.** For each active `n`: `c = act[n] × carry_spread / 1000`; if `c > 0`, `inc[n] = c`.
3. **Inject**, in list order: `a' = clamp(a, 0, A_CAP)` (flag if `a > A_CAP`); if `act[n] = 0` and
   `a' > 0`, `n` becomes active; `act[n] = min(act[n] + a', max_act)`; if `a' > 0`,
   `inc[n] = min(inc[n] + a', INC_CAP)`.
4. **Seed frontier.** Every node with `inc > 0` is a candidate with amount `min(inc, A_CAP)`
   (flag `SAT_FRONT` if it bound); `inc` is reset to 0. The frontier is the best `frontier`
   candidates by the tie order (§1).
5. **Hops**, at most `hops` times, stopping early when the frontier is empty. `hops_run` counts
   the hops run.
6. **Expand** each frontier sender `(u, a)`: `visits += deg(u)` (its links with positive weight);
   `den = max(Σ w, 1000)` over those links; `B` its bound (§4). Then its links **heaviest first**:
   - `q = a × w / den` (unsigned);
   - **early stop:** if `q × B < 1000 × min_act`, stop reading `u`'s links;
   - `flow = q × F / 1000` with `F` the link's factor (§4);
   - if `flow ≥ min_act`: `inc[t] = min(inc[t] + flow, INC_CAP)` (flag `SAT_INC` if it bound).
   The early stop is exact: `flow ≤ q × B / 1000 < min_act` for this link, and every later link
   has `w' ≤ w`, so `q' ≤ q`. The order of senders and of equal-weight links cannot matter, since
   saturating addition of non-negative numbers is `min(Σ, cap)` in any order.
7. **Take in the hop.** For every node `t` with `inc[t] > 0`: if `act[t] = 0`, `t` becomes active;
   `act[t] = min(act[t] + inc[t], max_act)`; it is a candidate with amount `min(inc[t], A_CAP)`
   (flag `SAT_FRONT`); `inc[t] = 0`. The next frontier is the best `frontier` candidates.
8. **Budget.** `total = Σ act` over active nodes. If `total > budget`, each becomes
   `act × budget / total`, and those below `min_act` are dropped.
9. **Cap.** If more than `active_cap` remain, keep the best `active_cap` by the tie order
   (amount = activation).
10. **Workspace**: the best `ws_k` of what remains, in tie order. The new state is what remains,
    sorted by node.

## 6. Why nothing overflows

| Product | Largest value | Fits |
|---|---|---|
| knots: `x × s × 2`, `x` ≤ 1000 at that point | 2·10⁶ | i32 |
| `x` after the tag steps | 16,000 | |
| `g0 × M−` (`M−` ≤ 5000) | 8·10⁷ | i32 |
| `… / 1000 × hd` | 8·10⁷ | i32 |
| interpolation `(F− − F0) × 1000` | 2.6·10⁸ | i32 |
| `F × fg`, then `× cg` (each ≤ 2000) | 5.2·10⁸, 1.05·10⁹ | i32 |
| `a × w` (`a` ≤ 16,383, `w` ≤ 1000) | 1.6·10⁷ | u32 |
| `q × F`, `q × B` (`q` ≤ 16,383, ≤ 262,143) | 4,294,688,769 | u32 (< 2³² = 4,294,967,296) |
| `inc + flow` (flow ≤ 4.3·10⁶) | 2³⁰ + 4.3·10⁶ | i32 |
| `act + inc` | 16,383 + 2³⁰ | i32 |
| `Σ act` | 16,383 × nodes | u32 for fewer than 262,144 active nodes |
| `act × budget` | 16,383 × 131,071 | u32 |
| `x × persist`, `act × carry` | 1.6·10⁷ | i32 |
| `1000 × min_act` (`min_act ≤ max_act ≤ A_CAP`) | 1.6·10⁷ | u32 |

The largest knot over all classes and modulators is 53,360 (wants into a food-, rest- and
anger-tagged target, every modulator at its extreme, `hop_decay` 1000; 29,348 at the default 550),
so with focus and courage it is at most 213,440: **`F_CAP` cannot bind** with today's gain table
(`tests/think_v2.rs` checks it). `A_CAP` does bind, rarely: in 11 of 31,274 thinks in a 3-day village a hop's sum into one
node was above 16,383.

## 7. Storage

**A species base, sorted** (`think2::SortedBase`, built once per `store::Base`, shared by every mind
of the species): for each node, its links with positive weight, **heaviest first** (then by their
position), its weight sum `den` (≥ 1000), and the mask of link types present. A link is 8 bytes:
target (32 bits); weight (10), class (7), link valence + 1024 (11). The link's position is kept on
the CPU only, for traces; the gap-coded base keeps the link keys.

**A mind's overlay, compiled in** (`think2::Compiled`, inside `store::Runtime`): every node that
differs from the base (a reweight or link valence in the overlay, a fast weight on any of its
links, a link the mind added) or that the mind added itself has a **patch**: its effective links
(weight = min(slow + fast, 1000), links with weight 0 left out), sorted heaviest first, with its
own sum and type mask. Other nodes read the shared base. The store marks a node dirty on every
change to its links (`set_slow`, `set_lval`, `set_fast`, `map_fast`, `add_link`, `remove_link`);
the next think recompiles only those. Fast weights stay sparse in the store (`Runtime::fast`); the
patch is derived from them, and is never hashed.

**On the GPU** (`think2::pack_base`, `pack_mind`, `pack_input`; all `u32` words):
- base: `[n, links, off(n+1), links(2 each), den(n), tmask(n), key(n)]`;
- mind: `[n, nbase, patches, patch links, patch_of(n), patch(4 each: first, count, den, tmask),
  patch links(2 each), valence(n, two i16 a word), keys of own nodes]`;
- a think: a 28-word header (mind, base, output offsets; 10 modulators; 10 params; state and
  injection list offsets and lengths) then its lists, 2 words an entry;
- output: `[flags, visits, hops_run, ws len, state len, ws_k, ws(ws_k), state(2 each)]`.

## 8. The GPU form

One dispatch per batch of thinks, **all hops inside it**. **One workgroup (64 threads) per think**;
each workgroup owns a scratch slot (`9 × stride` words, the stride fixed for the life of the
scratch buffer and at least the largest mind's node count: activation, hop sums, marks, the active,
touched and kept lists, candidates) and loops over thinks `wg, wg + W, …`, so a dispatch of `W`
workgroups runs any number of thinks. **Thinks are ordered by species base** before dispatch, so
neighbouring workgroups read the same base. The frontier (≤ 64) lives in workgroup memory.

- In parallel: the knots (one class a thread), persisting, expanding (one sender a thread),
  taking in a hop, and every selection. Selection is **rank counting**: an element's place is the
  number of elements that beat it in the tie order, which is a total order, so the result does
  not depend on thread timing.
- Sums into a node: `atomicAdd`; if the old value plus the flow passes `INC_CAP`, `atomicMin` to
  `INC_CAP`. The final value is `min(Σ, INC_CAP)` whatever the interleaving, because every addend
  is positive and each add that leaves the value above the cap is followed by its own min. The
  first add into a node (old value 0) puts it on the touched list.
- In order on thread 0: the injections (their order matters for activation's clamp) and the focus
  and courage marks.

## 9. A tick around the think

`Mind::live` runs a tick. If a think is **due** (thinking every tick, asleep, nothing chosen yet,
this mind's turn `(tick + phase) % every = 0`, or a **reflex**: pain rising, a creature in sight,
something within 2 cells approaching), the tick is `step` then `commit`; otherwise it is `idle`:
the senses are buffered, `upkeep` runs (fast weights fade every 32 ticks, volatility every 8,
familiarity, arousal −1/10, stress −1/40 − 1), and the body continues (walk, flee, charge, hold,
freeze, a nap) or stays.

A step (`Mind::step_with`), in order:

1. **`dt`** = ticks since the last think, 1..`MAX_DT` (64); 1 when thinking every tick. Buffered
   sights not in view now are merged in where last seen.
2. Hunger, hunger2 and fatigue (the consolidation backlog, or evening drowsiness) set the
   modulators. Asleep: stop. Dormant places at the body are revived. A nap continues
   (consolidation × `dt`, a damp of 900‰) unless woken.
3. **Sense and predict**: percepts and their prediction errors, absences, plan breaches; the
   **injection list** is built: each percept at `SEEN_TOUCH` (40) or its error (and half to its
   place), absences, the plan's broken node, hunger/2, fatigue/3 above 300, pain, the place
   underfoot at 300, heard cues, companions in alarm.
4. Arousal += surprise + pain rise; **damp**: the whole state × (1000 − max error × 700/1000)‰.
5. Focus: the held intent's carried activation (faded `persist^dt`) plus its rehearsal, × 2;
   courage: the ally node's activation × 4. Stress += its stressors (danger's share × `dt`).
6. **Params**: `stressed_params` (fewer hops, a smaller frontier under stress); `persist` becomes
   `persist^dt` (compounded per mille, truncating each time); under forced compaction the frontier
   shrinks.
7. **The think** (§5) on the state, with that list and those modulators.
8. After: stress falls; the intent lapses once out of the workspace for `INTENT_LAPSE` (3) ticks, counted in ticks (`dt`);
   **mood** moves towards the workspace's mean valence by 1/32, `dt` times; arousal += danger/64 ×
   `dt`, then −1/10.
9. Learning: buffered sights teach expectations and familiarity tick by tick; Hebbian learning
   on the workspace (× `dt`); extinction at absences (× `dt`); fast weights fade on their ticks.
   These are the changes that mark nodes dirty for the next think (§7).
10. **Choose**: danger responses, else `choose` (snap, flee, eat, then places by their look-ahead,
    §10); a rejected place's activation is damped by 500‰ in the state.
11. **Idle consolidation** when settled: budget `600 × dt − visits − forced`.

`Mind::imagine` (used by `home()` and the people layer) runs thinks from a cue on a fresh state,
without changing the mind; it goes through the same step.

## 10. The look-ahead (`Mind::felt_in`)

How a place `p` feels now, given the state after the think (`act`) and `top`, the activation of
the workspace's first node:

- `salience(n) = clamp(act(n), 0, top) × 1000 / top`; for what answers a need, the larger of the
  node's own and that of any `kind` target tagged food or rest.
- `good(val, n)` for a positive feeling: food- or rest-tagged `n`: `val × 2 × salience / 1000`; the
  comfort concept: `val × salience / 1000`; anything else: `val × (300 + arousal) / 1000`.
- `p`'s own valence: negative → `val × (1000 + 2 × arousal + low mood) / 1000`; else `good`.
- Each link of `p` with positive weight (source links skipped, `felt_without_sources`):
  `val = clamp(val(t) + lv, ±1000)`; a place target adds `w × val / 2000`; a negative one
  `w × val / 1000 × (1000 + 2 × arousal + low mood) / 1000`; a positive one `w × good(val, t) / 1000`.
- **The stick**, for a place: for each need (hunger → food, hunger2 → food2, fatigue → rest) whose
  body node feels negative and whose wanted thing has salience `s > 0`: if none of `p`'s links
  reaches the wanted thing directly or through a `kind` link, add `val(body) × s / 1000`.

`choose` scores a candidate place as its activation (plus pulls from seen things and things held
there) × the look-ahead (clamped at 0) / 1000, plus novelty, × the distance discount.

The look-ahead is not part of the step: it runs on the CPU after it, reading links through
`Form::edges`. Stage 1 found it is now the costliest part of a choice; making it a pass over the
sorted links (or a GPU pass) is left open.

## 11. What differs from v1

| | v1 (`think::think`) | v2 |
|---|---|---|
| Gain | per (class, valence), multiplier `2²⁰ + |v| × c`, 64-bit | three knots per class, line between, 32-bit |
| Flow | `((w × ((src ≪ 24) / Σw)) ≫ 24) × g × decay / 10⁶`, 64-bit | `(a × w / Σw) × F / 1000`, 32-bit |
| Overflow | 64-bit intermediates, unbounded i32 sums | saturating, flagged |
| Links | decoded from the gap-coded base, merged with three sparse lists, every link read | sorted base + compiled patches; stop at the first link that cannot flow |
| Injection of 0 | puts the node in the frontier with 0 | ignored |
| Duplicates in the state | possible (after a damp to 0) | each node once |
| Ablations | `Params::exact`, `index_ties`, `EARLY_STOP` | none (`think2::PRUNE` only for measuring) |
