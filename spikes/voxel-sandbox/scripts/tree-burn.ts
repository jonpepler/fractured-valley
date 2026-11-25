// How long does the big north tree take to burn? tsx scripts/tree-burn.ts
// Lights the grass at its foot and reports when the crown catches, how much
// is burning over time, when leaves are gone and when the trunk falls.
import { DEFAULT_SPEC, generate } from '../src/gen'
import { M } from '../src/materials'
import { initPhysics } from '../src/physics'
import { Sim } from '../src/sim'
await initPhysics()
const spec = DEFAULT_SPEC
const sim = new Sim(generate(spec), spec.seed)
const tx = Math.floor(spec.sx * 0.55), ty = Math.floor(spec.sy * 0.18)
let gz = spec.sz - 1; while (gz > 0 && sim.world.get(tx + 6, ty + 6, gz) === M.Air) gz--
sim.schedule({ tick: 1, peer: 0, seq: 0, command: { kind: 'ignite', x: tx + 6, y: ty + 6, z: gz } })
const count = (m: number) => { let n = 0; for (let z = 0; z < spec.sz; z++) for (let y = ty - 20; y < ty + 20; y++) for (let x = tx - 20; x < tx + 20; x++) if (sim.world.get(x, y, z) === m) n++; return n }
const leaves0 = count(M.Leaves), wood0 = count(M.Wood)
let leafCaught = -1, leavesHalf = -1, leavesGone = -1, fell = -1
for (let t = 0; t < 20 * 60 * 5 && (leavesGone < 0 || fell < 0); t++) {
  sim.step()
  if (t % 20) continue
  const l = count(M.Leaves)
  if (leafCaught < 0 && l < leaves0) leafCaught = sim.tick
  if (leavesHalf < 0 && l < leaves0 / 2) leavesHalf = sim.tick
  if (leavesGone < 0 && l < leaves0 / 20) leavesGone = sim.tick
  if (fell < 0 && sim.events.some(e => e.kind === 'detached' && e.cells > 100)) fell = sim.tick
}
const s = (t: number) => (t < 0 ? 'not within 5 min' : `${(t / 20).toFixed(0)} s`)
console.log({ leaves: leaves0, wood: wood0, firstLeafLost: s(leafCaught), halfLeavesGone: s(leavesHalf), crownBare: s(leavesGone), bigPieceFalls: s(fell), woodLeft: count(M.Wood) })
