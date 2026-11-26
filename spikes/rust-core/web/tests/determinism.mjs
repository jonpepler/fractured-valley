// node tests/determinism.mjs [--out <dir>] [browsers…]   (default: chromium firefox webkit)
// Runs the scenario in each browser's WebAssembly and checks every tick's
// hash equals the native build's (dist/native.json), and the books balance.
// --out also saves each run (and the native one) as JSON, in the format
// ci/determinism/compare.mjs reads, so CI can compare across machines.
import { mkdirSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import { firstDiff, native, pw, result, serve } from './lib.mjs'

const args = process.argv.slice(2)
const o = args.indexOf('--out')
const outDir = o >= 0 ? args.splice(o, 2)[1] : undefined
const names = args.length ? args : ['chromium', 'firefox', 'webkit']
const tag = `${os.platform()}-${os.arch()}`
const want = native()
const server = await serve(4181)
if (outDir) { mkdirSync(outDir, { recursive: true }); writeFileSync(`${outDir}/${tag}-native.json`, JSON.stringify({ ...want, platform: tag })) }
let failed = false
console.log(`native ${want.platform}: ${want.hashes.length} ticks, final ${want.hashes.at(-1)} (${want.msPerTick} ms/tick)`)
for (const name of names) {
  const t0 = Date.now()
  try {
    const browser = await pw[name].launch()
    const page = await browser.newPage()
    page.on('pageerror', e => console.error(`${name}: ${e.message}`))
    await page.goto(`${server.url}determinism.html`)
    const r = await result(page)
    const d = firstDiff(r.hashes, want.hashes)
    const ok = d === -1 && r.energyGap === '0' && r.massGap === '0'
    console.log(`${ok ? 'PASS' : 'FAIL'} ${name} ${browser.version()}: ${r.hashes.length} ticks, final ${r.hashes.at(-1)}, ${r.msPerTick.toFixed(2)} ms/tick, gaps ${r.energyGap}/${r.massGap}` +
      (d === -1 ? '' : `; first differs at tick ${d}: ${r.hashes[d]} vs native ${want.hashes[d]}`) + ` (${((Date.now() - t0) / 1000).toFixed(1)} s)`)
    if (!ok) failed = true
    if (outDir) writeFileSync(`${outDir}/${tag}-${name}.json`, JSON.stringify({ platform: tag, browser: name, version: browser.version(), ...r }, null, 2))
    await browser.close()
  } catch (e) {
    console.log(`FAIL ${name}: ${e.message}`)
    failed = true
  }
}
server.stop()
process.exit(failed ? 1 : 0)
