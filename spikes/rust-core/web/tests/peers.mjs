// node tests/peers.mjs [browser]          (default: chromium)
// Two real pages in one browser, in lockstep over BroadcastChannel. The host
// runs alone to tick 10; the joiner arrives late and starts from a
// checkpoint; then both split every tick's diffusion by brick, swap results
// as bytes, and spot-check a seeded quarter of each other's. Their hashes
// must equal the native solo run. Second run: the host sends one wrong value
// at tick 30, and the joiner must catch it that tick (and repair it).
// (The host hands over at tick 10, or later if pieces are falling then:
// rigid bodies are not in checkpoints yet. In this scenario that is tick 22.)
import { native, pw, result, serve } from './lib.mjs'

const name = process.argv[2] ?? 'chromium'
const server = await serve(4183)
const want = native().hashes
const fails = []
const check = (ok, what) => { console.log(`${ok ? 'PASS' : 'FAIL'} ${what}`); if (!ok) fails.push(what) }

const browser = await pw[name].launch()

async function pair(label, corrupt) {
  // One context, so both pages share an origin's BroadcastChannel.
  const ctx = await browser.newContext()
  const room = `${label}-${Date.now()}`
  const host = await ctx.newPage(), join = await ctx.newPage()
  for (const [n, p] of [['host', host], ['join', join]]) p.on('pageerror', e => console.error(`${n}: ${e.message}`))
  const t0 = Date.now()
  await host.goto(`${server.url}peers.html?role=host&room=${room}&joinAt=10&ticks=40${corrupt === undefined ? '' : `&corrupt=${corrupt}`}`)
  await host.waitForFunction(() => window.fvStatus?.startsWith('waiting'), null, { timeout: 60_000 })
  await join.goto(`${server.url}peers.html?role=join&room=${room}`)
  const [h, j] = await Promise.all([result(host), result(join)])
  const secs = ((Date.now() - t0) / 1000).toFixed(1)
  const matches = r => r.hashes.every(x => x.hash === want[x.tick])
  console.log(`-- ${label} (${name}): host ${h.hashes.length} ticks, joiner from tick ${j.firstSplitTick}, ${h.msPerSplitTick.toFixed(1)} ms per split tick, ${(h.bytesSent / (40 - h.firstSplitTick) / 1e6).toFixed(2)} MB sent per peer per tick, ${secs} s`)
  check(j.joinHashMatched === true, `${label}: joiner's state after import has the host's hash`)
  const split = 40 - h.firstSplitTick
  // The host joins at tick 10, or later if pieces were falling then.
  check(j.firstSplitTick === h.firstSplitTick && h.firstSplitTick >= 10 && split > 10, `${label}: split from tick ${h.firstSplitTick}`)
  check(matches(h) && h.hashes.length === 40, `${label}: host's 40 hashes == native solo`)
  check(matches(j) && j.hashes.length === split, `${label}: joiner's ${split} hashes == native solo`)
  check(h.peerHashMismatches.length === 0 && j.peerHashMismatches.length === 0, `${label}: peers agreed on every hash`)
  check(h.bricksSent === split * 32 && j.bricksSent === split * 32, `${label}: each computed half the bricks (${h.bricksSent}, ${j.bricksSent})`)
  check(h.checked > 0 && j.checked > 0, `${label}: spot-checked ${h.checked} and ${j.checked} of the other's bricks`)
  check(h.energyGap === '0' && h.massGap === '0' && j.energyGap === '0' && j.massGap === '0', `${label}: books balance on both`)
  check(h.caught.length === 0, `${label}: host caught nothing (joiner honest)`)
  await ctx.close()
  return j
}

const honest = await pair('honest')
check(honest.caught.length === 0, 'honest: joiner caught nothing')
const cheat = await pair('corrupt', 30)
console.log(`  joiner caught: ${JSON.stringify(cheat.caught)}`)
check(cheat.caught.length === 1 && cheat.caught[0].tick === 30, 'corrupt: the joiner caught the wrong value at tick 30, and only then')

await browser.close()
server.stop()
process.exit(fails.length ? 1 : 0)
