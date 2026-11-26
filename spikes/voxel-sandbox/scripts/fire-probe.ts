// Light the grass by the post and print how the fire develops: tsx scripts/fire-probe.ts [ticks]
import { DEFAULT_SPEC, generate } from '../src/gen'
import { M } from '../src/materials'
import { initPhysics } from '../src/physics'
import { Sim } from '../src/sim'
await initPhysics()
const spec = DEFAULT_SPEC
const sim = new Sim(generate(spec), spec.seed)
const px = Math.floor(spec.sx * 0.52), py = Math.floor(spec.sy * 0.56)
let gz = spec.sz - 1; while (gz > 0 && sim.world.get(px - 4, py - 4, gz) === M.Air) gz--
sim.schedule({ tick: 1, peer: 0, seq: 0, command: { kind: 'ignite', x: px - 4, y: py - 4, z: gz } })
const ticks = Number(process.argv[2] ?? 1200)
let ms = 0
for (let t = 1; t <= ticks; t++) {
  const s = performance.now(); sim.step(); ms += performance.now() - s
  if (t % 100 === 0) {
    let hot = 0, gas = 0
    for (const [, , , h, so, st] of sim.fields.cells()) { gas++; if (h > 260) hot++ }
    console.log(`t=${t / 20}s burning=${sim.last.burning} gasCells=${gas} hot=${hot} embers=${sim.embers.length} activeBricks=${sim.fields.active.size} ms/tick=${(ms / 100).toFixed(2)}`)
    ms = 0
  }
}
console.log(sim.events.filter(e => e.kind !== 'crumbled' && e.kind !== 'gave-way').slice(-12).map(e => `${e.tick} ${e.kind} ${e.cells}`).join(' | '))
