// node tests/speed.mjs [ticks]   (default 100)
// The dead-tree scene in Chromium, single-threaded and on 4 Web Workers: how
// many ms a tick takes (real time is 50), and every tick's hash must equal
// the native build's (dist/native-tree.json, written by scripts/build.sh).
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { WEB, firstDiff, pw, result, serve } from './lib.mjs'

const ticks = Number(process.argv[2] ?? 100)
const want = JSON.parse(readFileSync(join(WEB, 'dist/native-tree.json'), 'utf8'))
if (want.hashes.length < ticks) throw new Error(`dist/native-tree.json has only ${want.hashes.length} ticks`)
const server = await serve(4183)
const browser = await pw.chromium.launch()
const page = await browser.newPage()
page.on('pageerror', e => console.error(`page: ${e.message}`))
await page.goto(`${server.url}speed.html?threads=4&ticks=${ticks}`)
const r = await result(page, 1_800_000)
const fails = []
const check = (ok, what) => { console.log(`${ok ? 'PASS' : 'FAIL'} ${what}`); if (!ok) fails.push(what) }
check(firstDiff(r.st.hashes, want.hashes.slice(0, ticks)) === -1, `single-threaded wasm == native over ${ticks} ticks`)
check(firstDiff(r.mt.hashes, want.hashes.slice(0, ticks)) === -1, `threaded wasm on ${r.threads} workers == native over ${ticks} ticks`)
console.log(`  dead tree, ms/tick: native ${want.msPerTick.toFixed(1)} (${want.platform}), wasm single-threaded ${r.st.msPerTick.toFixed(1)}, wasm ${r.threads} workers ${r.mt.msPerTick.toFixed(1)} (Chromium ${browser.version()}, ${r.hardwareConcurrency} cores)`)
await browser.close()
server.stop()
process.exit(fails.length ? 1 : 0)
