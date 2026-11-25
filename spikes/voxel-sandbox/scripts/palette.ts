// How small do surface bricks get with a per-brick palette?
// A brick that uses ≤2 materials needs 1 bit per cell, ≤4 → 2 bits,
// ≤16 → 4 bits, plus the palette itself. Cell state is sparse (only burning
// or damaged cells), so it is stored as a list, not an array.
import { generate } from '../src/gen'

const g = generate()
let bricks = 0, raw = 0, packed = 0
const hist = new Map<number, number>()
for (const b of g.bricks) {
  if (!b) continue
  bricks++
  const used = new Set(b.material)
  const bits = used.size <= 1 ? 0 : used.size <= 2 ? 1 : used.size <= 4 ? 2 : used.size <= 16 ? 4 : 8
  let stated = 0
  for (const s of b.state) if (s) stated++
  hist.set(used.size, (hist.get(used.size) ?? 0) + 1)
  raw += 4096 * 6
  packed += (4096 * bits) / 8 + used.size * 2 + stated * 4 + 8
}
console.log({ bricks, materialsPerBrick: Object.fromEntries([...hist].sort((a, b) => a[0] - b[0])) })
console.log(`raw ${(raw / 1048576).toFixed(1)} MiB → palette-packed ${(packed / 1048576).toFixed(2)} MiB (${(raw / packed).toFixed(1)}×)`)
