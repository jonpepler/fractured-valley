// Runs the fixed scenario and publishes its hashes on window.result,
// for the cross-browser check (ci/determinism).
import { initPhysics } from './physics'
import { IsoRenderer } from './render'
import { runScenario } from './scenario'

await initPhysics()
const { sim, result } = runScenario()
const renderer = new IsoRenderer(sim.world)
renderer.seed = sim.seed
renderer.render(sim.grids, sim.tick)
let h = 0x811c9dc5
for (const v of new Uint32Array(renderer.image.data.buffer)) h = Math.imul(h ^ v, 0x01000193) >>> 0
const out = { ...result, pixelHash: h.toString(16).padStart(8, '0'), userAgent: navigator.userAgent }
Object.assign(window, { result: out })
document.querySelector('pre')!.textContent = JSON.stringify(out, null, 2)
