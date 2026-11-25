import { DEFAULT_SPEC, generate } from './gen'
import { mat } from './materials'
import { IsoRenderer } from './render'
import { type Command, Sim } from './sim'

const TICK_MS = 50 // 20 Hz simulation
const INPUT_DELAY = 2 // ticks: commands are scheduled ahead, as lockstep will need

const params = new URLSearchParams(location.search)
const spec = { ...DEFAULT_SPEC, seed: Number(params.get('seed') ?? DEFAULT_SPEC.seed) }

const t0 = performance.now()
const world = generate(spec)
const genMs = performance.now() - t0
const sim = new Sim(world, spec.seed)
const renderer = new IsoRenderer(world)

const canvas = document.querySelector<HTMLCanvasElement>('#view')!
const ctx = canvas.getContext('2d')!
canvas.width = renderer.width
canvas.height = renderer.height
const hud = document.querySelector<HTMLPreElement>('#hud')!
const viewport = document.querySelector<HTMLDivElement>('#viewport')!

// ── camera: integer zoom, drag to pan ──────────────────────────────────────
let zoom = Number(params.get('zoom') ?? 2)
let panX = 0
let panY = 0
function fit() {
  panX = Math.round((viewport.clientWidth - renderer.width * zoom) / 2)
  panY = Math.round((viewport.clientHeight - renderer.height * zoom) / 2)
  applyCamera()
}
function applyCamera() {
  canvas.style.transform = `translate(${panX}px, ${panY}px) scale(${zoom})`
}

// ── input ───────────────────────────────────────────────────────────────────
let seq = 0
function issue(command: Command) {
  sim.schedule({ tick: sim.tick + INPUT_DELAY, peer: 0, seq: seq++, command })
}

function cellAt(ev: MouseEvent): [number, number, number] | undefined {
  const r = canvas.getBoundingClientRect()
  const x = Math.floor((ev.clientX - r.left) / zoom)
  const y = Math.floor((ev.clientY - r.top) / zoom)
  if (x < 0 || y < 0 || x >= renderer.width || y >= renderer.height) return undefined
  const p = renderer.pick[y * renderer.width + x]!
  if (p < 0) return undefined
  return [p & 1023, (p >> 10) & 1023, p >>> 20]
}

let drag: { x: number; y: number; px: number; py: number; moved: boolean } | undefined
viewport.addEventListener('pointerdown', ev => {
  drag = { x: ev.clientX, y: ev.clientY, px: panX, py: panY, moved: false }
})
window.addEventListener('pointermove', ev => {
  hover = cellAt(ev)
  if (!drag) return
  const dx = ev.clientX - drag.x, dy = ev.clientY - drag.y
  if (Math.abs(dx) + Math.abs(dy) > 4) drag.moved = true
  if (drag.moved) { panX = drag.px + dx; panY = drag.py + dy; applyCamera() }
})
window.addEventListener('pointerup', ev => {
  const wasDrag = drag?.moved
  drag = undefined
  if (wasDrag) return
  const cell = cellAt(ev)
  if (!cell) return
  const [x, y, z] = cell
  issue(ev.button === 2 ? { kind: 'drop', x, y, z } : { kind: 'ignite', x, y, z })
})
viewport.addEventListener('contextmenu', ev => ev.preventDefault())
viewport.addEventListener('wheel', ev => {
  ev.preventDefault()
  const next = Math.max(1, Math.min(6, zoom + (ev.deltaY < 0 ? 1 : -1)))
  if (next === zoom) return
  // Zoom around the cursor.
  const r = viewport.getBoundingClientRect()
  const cx = ev.clientX - r.left, cy = ev.clientY - r.top
  panX = Math.round(cx - ((cx - panX) * next) / zoom)
  panY = Math.round(cy - ((cy - panY) * next) / zoom)
  zoom = next
  applyCamera()
}, { passive: false })

let paused = false
window.addEventListener('keydown', ev => {
  if (ev.key === ' ') { paused = !paused; ev.preventDefault() }
  if (ev.key === '.') stepOnce()
  if (ev.key === 'f') fit()
})

// ── loop ────────────────────────────────────────────────────────────────────
let hover: [number, number, number] | undefined
let simMs = 0
let renderMs = 0
let lastHash = sim.hash()
let surfaceCells = 0
let acc = 0
let last = performance.now()

function stepOnce() {
  const s = performance.now()
  sim.step()
  simMs = simMs * 0.9 + (performance.now() - s) * 0.1
  if (sim.tick % 20 === 0) lastHash = sim.hash()
  dirty = true
}

let dirty = true
function frame(now: number) {
  acc += Math.min(250, now - last)
  last = now
  if (!paused) while (acc >= TICK_MS) { stepOnce(); acc -= TICK_MS }
  else acc = 0
  if (dirty) {
    const s = performance.now()
    surfaceCells = renderer.render(sim.grids, sim.tick).surfaceCells
    ctx.putImageData(renderer.image, 0, 0)
    renderMs = renderMs * 0.9 + (performance.now() - s) * 0.1
    dirty = false
  }
  drawHud()
  requestAnimationFrame(frame)
}

function drawHud() {
  const st = world.stats()
  const l = sim.last
  const lines = [
    `tick ${sim.tick}${paused ? ' (paused)' : ''}   hash ${lastHash.toString(16).padStart(8, '0')}`,
    `sim ${simMs.toFixed(2)} ms   render ${renderMs.toFixed(1)} ms   gen ${genMs.toFixed(0)} ms`,
    `burning ${l.burning}   loose ${l.loose}   bodies ${l.bodies}   surface ${surfaceCells}`,
    `bricks ${st.detailed}/${st.bricks} detailed   ${(st.bytes / 1048576).toFixed(1)} MB for ${(st.cells / 1e6).toFixed(1)}M cells`,
  ]
  if (hover) {
    const [x, y, z] = hover
    lines.push(`cell ${x},${y},${z}  ${mat(world.get(x, y, z)).name}  state ${world.getState(x, y, z)}`)
  }
  hud.textContent = lines.join('\n')
}

// Inspectable from the console (pillar 6).
Object.assign(window, { fv: { sim, world, renderer, spec, issue, step: stepOnce } })

fit()
requestAnimationFrame(frame)
