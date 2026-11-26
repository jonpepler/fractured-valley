# Spike: two brains talking

People hold conversations and learn about each other across short-term, long-term and emotional memory, and their relationship. Headless, deterministic, integer maths only, no dependencies. Spike 5 of the AI-brains spec. Not the engine.

```bash
cargo test                                               # 8 tests
cargo run --release --example valley_days > trace.txt    # 8 days with Ada, Bo and Cal
```

`trace.txt` is a saved run: every utterance, why it was chosen, what each brain heard, learned and recalled, with a memory snapshot after each day.

## What the tests show

The tests script world events (where people are, who is hungry or hurt) and starting knowledge. They never script a line of dialogue: every utterance comes from a brain's own choice.

| Test | What happens | Key numbers |
|---|---|---|
| `learns_a_fact_about_the_other` | Ada says she likes flowers. Bo stores `likes(Ada, flowers)`, told by Ada. He also derives that she'd like the meadow. | It survives once short-term memory is empty. With retrospection off, the same talk leaves nothing behind. |
| `teaching_the_same_fact_in_another_form` | Ada holds `is_in(food, the cold box)`. Bo is taught `holds(the cold box, food)`. Hungry, he walks there and eats. | Both choose the cold box via different memory forms. After a night both have compiled the same habit. |
| `trust_falls_after_a_contradicted_claim` | A sly, hungry Ada answers "the well holds food". Bo goes and sees water. | Trust in Ada falls from 460 to 184, and a Betrayed episode is stored. Next day she tells the truth. Bo holds it at confidence 147, below the 250 needed to act, and stays hungry. An honest Ada is believed. |
| `a_bad_episode_colours_a_later_talk` | Hurt and irritable, Ada snarls at Bo on day 1. On day 4, with nothing left in short-term memory, Bo recalls the episode. | Bo's greeting warmth is -14 against 551. He leaves after 6 ticks against 12, and laughs 0 times against 2. |
| `friendship_grows_and_changes_what_is_said` | Six morning meetings. | Familiarity 200→737, affinity 134→1000. Strangers introduce themselves; friends ask "How are you?", laugh more and greet warmly. |
| `identical_runs_and_any_brain_order_give_identical_state` | Three days with three brains, one of them a liar. | Same state hash twice. The same hash and trace when brain order changes every tick. The hash is pinned. |
| `quiet_voices_are_missed_or_misheard_deterministically` | At 18 m, "The cold box holds food." can be missed, heard, or misheard as "…holds flowers." | All three decided by a hash roll. |
| `a_third_person_overhears` | Cal stands 5 m away, outside talking range. He overhears Ada tell Bo where the food is. | Cal stores it as `overheard Ada`, without ever speaking or starting a conversation. A Cal 40 m away learns nothing. |

## Design

- **Each tick**, every brain reads the same view of the previous tick: bodies, places in sight, and the sounds in the air. It writes only its own state and one action. The world applies the actions afterwards, so brain order cannot matter.
- **Utterances are world events, not messages.** An utterance is the brain's content, said aloud: a `Claim(subject, predicate, object, confidence)`, `Ask(pattern)`, `Greet(warmth)`, `Emote(valence, arousal)` (laugh, snarl), `DontKnow`, `Refuse` or `Farewell`.
  - It goes into the air with a medium (only `Voice` so far), a volume and a position. `facing` is only a cue that listeners perceive, not an address.
  - Anyone in earshot perceives it. A claim from someone facing me is `told by`; otherwise it is `overheard`.
  - English exists only in the trace.
- **Sound:** loudness is `vol·100/(100+d²)`. Below 60 nothing is heard; at 300 or more everything is heard. In between, `hash(seed, tick, listener, salt)` decides between missed, misheard (an object becomes a sound-alike word) and clear. Speakers raise their volume for distance, up to their voice's limit.
- **Choosing what to say:** utility over about a dozen candidates, scored from personality (honesty, warmth, talkativeness, irritability, trust, curiosity) and needs (hunger, pain). Scores also use the relationship, the feeling recalled from emotional memory, the current mood, and what I think they already know. Each brain's trace prints the winner and the next two.
  - A lie wins when honesty is low, I'm hungry, and the asker isn't a friend.
  - A snarl wins when irritable and in pain.
  - Goodbye grows with the length of the talk and with coldness.
- **Memory layers:**
  - **Short-term:** 16 traces (heard, said, seen, ate, felt). Strength falls 8 per tick, so about 125 ticks; high-arousal traces last up to twice as long.
  - **Long-term:** up to 48 beliefs. Each holds a fact in the form it was learned, plus confidence, source (founding / seen / told by X / overheard X / derived) and time. Told confidence is the stated confidence × trust in the speaker.
  - **Emotional:** episodes holding who, valence, arousal, strength and cause. They fade by 1/8 per night. Meeting someone recalls the episodes with them as one feeling.
  - **Habits:** compiled `hungry → place` rules, the fast path. Food is looked up habit first, then long-term beliefs in either form, then things just heard.
  - **Relationship per person:** familiarity, trust, affinity, meetings, the facts I think they know (theory of mind lite), and likes we share.
- **Seeing tests what you were told.** A seen fact that contradicts a told or overheard belief removes the belief, multiplies trust in the teller by 0.4 and adds a Betrayed feeling. A seen fact that confirms one makes it a seen belief and raises trust a little.
- **Retrospection** runs after each conversation, every 100 ticks while idle (staggered per brain), and on sleep.
  - It turns short-term traces into beliefs, episodes and relationship changes, and derives simple beliefs ("likes flowers" + "the meadow holds flowers" → "likes the meadow").
  - On sleep it also compiles habits from strong beliefs and lets episodes fade.
  - Then short-term memory is cleared. Whatever decays before a pass is lost.

## What's faked

- The world is a line with three places, and each place holds one thing. Bodies teleport where the script says; brains only walk, for food.
- Nobody explores. A hungry Bo who distrusts the only person who told him stays hungry.
- Weights and thresholds are hand-tuned constants. Nothing is derived (unlike decision 4.2).
- Conversations are one to one; others can only overhear. Overlapping speech isn't modelled: two people can talk at once.
- The vocabulary is a fixed enum of things and five predicates. There's no grammar, no negation, no "I don't know where X is, ask Y".
- Episodes keep piling up: after 50 daily meetings Bo holds 39 of them. They should be merged into the relationship over time.

## What it suggests for the real design

1. **Knowledge as (fact, form, source, confidence) works**, and "same fact, different form" can be real and cheap. Agents share one spoken form (`holds`), and each keeps its own stored form. Speaking translates the stored form into the spoken one and storing keeps it as heard; behaviour reads both. Partial or wrong teaching comes for free from mishearing and from lies.
2. **Retrospection has to be periodic, not only at sleep.** With short-term memory lasting about 125 ticks, an overheard fact would be gone by night. The pass is cheap enough to run every few seconds of game time, staggered across brains.
3. **Trust as a multiplier on told confidence** is enough for lies to have consequences. Testing claims against your own senses is the only route to trust.
   - Lies spread by overhearing. In the 8-day run, Ada overhears Cal's lie and still believes it at the end, because she has never been to the well.
   - Gossip and factions will need trust per source *and* per topic.
4. **Emotional memory needs its own decay and a merge step.** Feeling recalled at a meeting (e.g. -386 four days after a snarl) visibly shapes the talk. Forgiveness is slow: the snarl episode still dominates on day 8. Its decay rate is a design choice the owner should feel out.
5. **Sound outside the cell system** is a good fit. An utterance is a small event at a point with a volume. Listeners sample it from last tick's air, so it is read-previous/write-own, and nothing about communication needs a group object.
6. **Cost is tiny.** Measured on this machine with tracing off:
   - about 10 µs per conversation (two brains, ~15 ticks);
   - about 350 ns per brain per talking tick;
   - about 80 ns per brain per idle tick.

   Each brain holds about 2 KB of memory. Talk won't be the budget; the senses and the world will.
