// The determinism scenario outside a browser: tsx scripts/determinism.ts
import { initPhysics } from '../src/physics'
import { runScenario } from '../src/scenario'
await initPhysics()
const { result } = runScenario()
console.log(JSON.stringify({ platform: `${process.platform}-${process.arch}`, browser: 'node', version: process.version, ...result }))
