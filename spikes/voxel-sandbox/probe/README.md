# Probes

Measurement scripts, not tests: they print numbers for tuning and assert nothing.
They sit outside `test/` so the normal suite doesn't run them.

- `flame-height.test.ts`: flame and unburnt vapour by height above ground, and the burnt area, every 5 s of the post scenario.
- `post.test.ts`: what happens to the test post's wood (burning, moisture) and its structural events.

Run one with `npx vitest run --root . probe/<name>.test.ts --silent=false`.
- `pile.test.ts`: a crib of dry logs against the big north tree; flame by height, burning cells and hottest air, every 7.5 s.
- `rest.test.ts`: the post test's scenario run on to 70 s, printing which bodies are still falling and when they land.
