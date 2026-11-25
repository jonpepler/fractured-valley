// node compare.mjs <dir>: every result must match the first, hash for hash.
import { readdirSync, readFileSync } from 'node:fs'
const dir = process.argv[2]
const runs = readdirSync(dir, { recursive: true }).filter(f => String(f).endsWith('.json'))
  .map(f => JSON.parse(readFileSync(`${dir}/${f}`, 'utf8'))).sort((a, b) => `${a.platform}${a.browser}`.localeCompare(`${b.platform}${b.browser}`))
if (!runs.length) { console.error('no results'); process.exit(1) }
const ref = runs[0]
let ok = true
const rows = ['| platform | engine | final hash | pixels | ms/tick | matches |', '|---|---|---|---|---|---|']
for (const r of runs) {
  const firstDiff = r.hashes.findIndex((h, i) => h !== ref.hashes[i])
  const pixels = r.pixelHash === undefined || ref.pixelHash === undefined || r.pixelHash === ref.pixelHash
  const same = firstDiff < 0 && r.hashes.length === ref.hashes.length && pixels
  if (!same) ok = false
  const where = firstDiff >= 0 ? `no: differs from check ${firstDiff}` : !pixels ? 'no: pixels differ' : 'yes'
  rows.push(`| ${r.platform} | ${r.browser} ${r.version ?? ''} | ${r.hashes.at(-1)} | ${r.pixelHash ?? 'n/a'} | ${r.msPerTick.toFixed(2)} | ${where} |`)
}
console.log(rows.join('\n'))
if (!ok) {
  console.log('\nEvents of the first mismatch vs reference:')
  const bad = runs.find(r => r.hashes.some((h, i) => h !== ref.hashes[i]))
  if (bad) console.log(JSON.stringify({ ref: ref.events, bad: bad.events }, null, 1))
}
process.exit(ok ? 0 : 1)
