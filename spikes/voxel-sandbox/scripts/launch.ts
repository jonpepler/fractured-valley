// Throw a metal cube at a tree trunk and report what happens:
// tsx scripts/launch.ts [speed cells/s]
import { DEFAULT_SPEC, generate } from '../src/gen'
import { M } from '../src/materials'
import { initPhysics } from '../src/physics'
import { Sim } from '../src/sim'
await initPhysics()
const speed = Number(process.argv[2] ?? 120)
const spec = DEFAULT_SPEC
const sim = new Sim(generate(spec), spec.seed)
const tx = Math.floor(spec.sx * 0.86), ty = Math.floor(spec.sy * 0.42)
let gz = spec.sz - 1; while (gz > 0 && sim.world.get(tx + 22, ty, gz) === M.Air) gz--
sim.schedule({ tick: 2, peer: 0, seq: 0, command: { kind: 'launch', x: tx + 22, y: ty, z: gz + 6, vx: -speed, vy: 0, vz: 10 } })
for (let i = 0; i < 300; i++) { sim.step(); const b = sim.bodies[0]; if (process.env.TRACE && b && i < 30) { const p = b.grid.pose!; console.log(sim.tick, p.x.toFixed(1), p.y.toFixed(1), p.z.toFixed(1)) } }
console.log(sim.events.map(e => `${e.tick} ${e.kind} ${e.cells} @${e.at}`).join('\n'))
