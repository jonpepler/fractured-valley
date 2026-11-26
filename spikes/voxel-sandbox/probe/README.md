# Probes

Measurement scripts, not tests: they print numbers for tuning and assert nothing.
They sit outside `test/` so the normal suite doesn't run them.

- `flame-height.test.ts`: flame and unburnt vapour by height above ground, and the burnt area, every 5 s of the post scenario.
- `post.test.ts`: what happens to the test post's wood (burning, moisture) and its structural events.

Run one with `npx vitest run --root . probe/<name>.test.ts --silent=false`.
