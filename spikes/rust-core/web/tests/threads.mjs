// node tests/threads.mjs
// The threaded build on 4 Web Workers, in Chromium, served by a static server
// that sends no COOP/COEP headers. The service worker must make the page
// cross-origin isolated (after one reload), and the threaded hashes must
// equal the single-threaded and native ones.
import { firstDiff, native, pw, result, serve } from './lib.mjs'

const server = await serve(4182)
const want = native()
const fails = []
const check = (ok, what) => { console.log(`${ok ? 'PASS' : 'FAIL'} ${what}`); if (!ok) fails.push(what) }

// The server really sends no isolation headers.
const head = await fetch(`${server.url}threads.html`)
check(!head.headers.has('cross-origin-opener-policy') && !head.headers.has('cross-origin-embedder-policy'), 'server sends no COOP/COEP headers')

const browser = await pw.chromium.launch()
const page = await browser.newPage()
page.on('pageerror', e => console.error(`page: ${e.message}`))
page.on('console', m => { if (/COOP|error/i.test(m.text())) console.log(`  console: ${m.text()}`) })
const loads = []
page.on('response', r => { if (r.request().isNavigationRequest()) loads.push({ fromServiceWorker: r.fromServiceWorker(), coep: r.headers()['cross-origin-embedder-policy'] ?? null }) })
const t0 = Date.now()
await page.goto(`${server.url}threads.html?threads=4`)
// Each tick got heavier (flames above the bed, the air carrying its own motion):
// the three 60-tick runs plus the 64×64×32 timing take several minutes in CI.
const r = await result(page, 900_000)
const secs = ((Date.now() - t0) / 1000).toFixed(1)

console.log(`  navigations: ${JSON.stringify(loads)}`)
check(loads.length >= 2 && loads[0].coep === null, 'first load had no COEP header, then the page reloaded')
check(loads.at(-1).fromServiceWorker && loads.at(-1).coep === 'require-corp', 'final load came through the service worker with COEP added')
check(r.crossOriginIsolated === true, 'crossOriginIsolated is true')
check(r.threads === 4, `ran with ${r.threads} workers (hardwareConcurrency ${r.hardwareConcurrency})`)
check(firstDiff(r.st, want.hashes) === -1, 'single-threaded wasm == native')
check(firstDiff(r.mt1, want.hashes) === -1, 'threaded build, pool off == native')
check(firstDiff(r.mt, want.hashes) === -1, 'threaded build on 4 workers == native')
const t = r.msPerTick
console.log(`  ms/tick (32³): st ${t.st.toFixed(2)}, mt pool off ${t.mt1.toFixed(2)}, mt 4 workers ${t.mt.toFixed(2)}; 64×64×32: 1 thread ${t.big64Single.toFixed(1)}, 4 workers ${t.big64Threaded.toFixed(1)}; pool start ${r.poolMs.toFixed(0)} ms; total ${secs} s`)
await browser.close()
server.stop()
process.exit(fails.length ? 1 : 0)
