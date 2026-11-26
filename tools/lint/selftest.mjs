// The lint must prove it can still see (Gonogo's rule: a check that can only
// report zero is not a gate). Lints a fixture with exactly 4 planted `else`
// clauses and fails unless the plugin reports exactly 4.
import { execFileSync } from 'node:child_process'
const out = (() => {
  try {
    return execFileSync('npx', ['biome', 'lint', '--reporter=json', '--max-diagnostics=none', 'tools/lint/fixtures/planted-else.ts'], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] })
  } catch (e) {
    return e.stdout // biome exits non-zero when it finds errors, which it must
  }
})()
const found = JSON.parse(out).diagnostics.filter(d => d.category === 'plugin').length
if (found !== 4) {
  console.error(`no-else plugin saw ${found} of 4 planted else clauses: the lint is blind`)
  process.exit(1)
}
console.log('no-else plugin sees all 4 planted else clauses')
