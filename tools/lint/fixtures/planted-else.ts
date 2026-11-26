// Planted for tools/lint/selftest.mjs: exactly 4 else clauses, of every shape.
export function a(x: number) {
  if (x > 1) { return 1 } else { return 2 }
}
export function b(x: number) {
  if (x > 1) return 1
  else return 2
}
export function c(x: number) {
  if (x > 1) { x++ } else if (x < 0) { x-- } else { x = 0 }
  return x
}
