// Determinism page: run the scenario in the single-threaded build and publish
// the hash list on `window.result` for the test to compare with native.

import { SCENARIO, load, run, scenario } from './fv'

declare global { interface Window { result?: unknown; error?: string } }

const out = document.getElementById('out')!
try {
  const fv = await load('st')
  const e = scenario(fv)
  const { hashes, msPerTick } = run(e, SCENARIO.ticks)
  window.result = { scenario: SCENARIO, hashes, msPerTick, energyGap: e.energy_gap(), massGap: e.mass_gap(), userAgent: navigator.userAgent }
  out.textContent = `${SCENARIO.ticks} ticks, ${msPerTick.toFixed(2)} ms/tick\nfinal hash ${hashes.at(-1)}\nenergy gap ${e.energy_gap()}, mass gap ${e.mass_gap()}`
} catch (err) {
  window.error = String(err)
  out.textContent = `error: ${err}`
}
