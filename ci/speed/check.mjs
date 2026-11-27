// node ci/speed/check.mjs [--record] [--wasm]
// Times every spike scene natively (fv-core/examples/speed.rs) and, with
// --wasm, the dead tree in both WebAssembly builds (web/tests/speed.mjs; run
// web/scripts/build.sh first). Fails if any scene is more than 2× slower than
// the time recorded for this machine in baseline.json. --record writes this
// run's times as the new baseline for the machine.
//
// The machine is named by SPEED_MACHINE (default: the OS, architecture and
// core count), since times only compare on the same hardware.
import { execFileSync } from 'node:child_process'
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import os from 'node:os'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const core = join(here, '../../spikes/rust-core')
const args = process.argv.slice(2)
const machine = process.env.SPEED_MACHINE ?? `${os.platform()}-${os.arch()}-${os.cpus().length}cores`

const native = JSON.parse(execFileSync('cargo', ['run', '--quiet', '--release', '-p', 'fv-core', '--example', 'speed'], { cwd: core, encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'] }))
const times = Object.fromEntries(Object.entries(native.msPerTick).map(([k, v]) => [`native ${k}`, v]))
if (args.includes('--wasm')) {
  const out = execFileSync('node', ['tests/speed.mjs', '60'], { cwd: join(core, 'web'), encoding: 'utf8' })
  const m = out.match(/wasm single-threaded ([\d.]+), wasm (\d+) workers ([\d.]+)/)
  if (!m) throw new Error(`no timings in the speed test's output:\n${out}`)
  times['wasm dead-tree single-threaded'] = Number(m[1])
  times[`wasm dead-tree ${m[2]} workers`] = Number(m[3])
}

const file = join(here, 'baseline.json')
const all = existsSync(file) ? JSON.parse(readFileSync(file, 'utf8')) : {}
if (args.includes('--record')) {
  all[machine] = { recorded: new Date().toISOString().slice(0, 10), msPerTick: times }
  writeFileSync(file, JSON.stringify(all, null, 2) + '\n')
  console.log(`recorded the baseline for ${machine}`)
}
const base = all[machine]?.msPerTick
let failed = false
for (const [name, ms] of Object.entries(times)) {
  const was = base?.[name]
  const slow = was !== undefined && ms > 2 * was
  if (slow) failed = true
  console.log(`${slow ? 'FAIL' : 'ok  '} ${name}: ${ms.toFixed(1)} ms/tick` + (was === undefined ? ' (no baseline)' : ` (baseline ${was.toFixed(1)}, ${(ms / was).toFixed(2)}×)`) + (ms <= (name.startsWith('wasm') ? 50 : 25) ? '' : '; over the target'))
}
if (!base) console.log(`no baseline for ${machine}: run with --record to make one`)
process.exit(failed ? 1 : 0)
