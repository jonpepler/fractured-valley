import { DEFAULT_SPEC, generate } from './gen'
import { initPhysics } from './physics'
import { mat } from './materials'
import { IsoRenderer } from './render'
import { BroadcastTransport, Lockstep, SoloTransport, type Transport, TrysteroTransport } from './net'
import { type Command, Sim } from './sim'

const TICK_MS = 50 // 20 Hz simulation

const params = new URLSearchParams(location.search)
const spec = { ...DEFAULT_SPEC, seed: Number(params.get('seed') ?? DEFAULT_SPEC.seed) }

await initPhysics()
const t0 = performance.now()
const world = generate(spec)
const genMs = performance.now() - t0
const sim = new Sim(world, spec.seed)
const renderer = new IsoRenderer(world)

// ?net=broadcast (default: other tabs of this browser) | trystero (real P2P) | solo
const room = params.get('room') ?? 'valley'
const netKind = params.get('net') ?? 'broadcast'
const transport: Transport =
  netKind === 'trystero' ? new TrysteroTransport(room) : netKind === 'solo' ? new SoloTransport() : new BroadcastTransport(room)
const net = new Lockstep(transport, sim, spec.seed)
net.start(netKind === 'solo' ? 0 : netKind === 'trystero' ? 4000 : 600)

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
  const focus = params.get('focus')?.split(',').map(Number)
  if (focus && focus.length === 3) {
    // Centre the camera on a world cell, e.g. ?focus=99,107,50
    const [ax, ay] = renderer.anchor(focus[0]!, focus[1]!, focus[2]!)
    panX = Math.round(viewport.clientWidth / 2 - ax * zoom)
    panY = Math.round(viewport.clientHeight / 2 - ay * zoom)
  } else {
    panX = Math.round((viewport.clientWidth - renderer.width * zoom) / 2)
    panY = Math.round((viewport.clientHeight - renderer.height * zoom) / 2)
  }
  applyCamera()
}
function applyCamera() {
  canvas.style.transform = `translate(${panX}px, ${panY}px) scale(${zoom})`
}

// ── input ───────────────────────────────────────────────────────────────────
function issue(command: Command) {
  net.issue(command)
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

function stepOnce(): boolean {
  const s = performance.now()
  if (!net.advance()) return false
  simMs = simMs * 0.9 + (performance.now() - s) * 0.1
  if (sim.tick % 20 === 0) lastHash = sim.hash()
  dirty = true
  const capture = captures.get(sim.tick)
  if (capture) { captures.delete(sim.tick); capture(captureNow()) }
  return true
}

// Acceptance check 0.4: peers with the same camera render the same image.
// captureAt(tick) renders the world exactly at that tick and hashes the pixels.
const captures = new Map<number, (c: { tick: number; pixelHash: string; png: string }) => void>()
function captureNow() {
  renderer.render(sim.grids, sim.tick)
  let h = 0x811c9dc5
  for (const v of new Uint32Array(renderer.image.data.buffer)) h = Math.imul(h ^ v, 0x01000193) >>> 0
  const c = document.createElement('canvas')
  c.width = renderer.width; c.height = renderer.height
  c.getContext('2d')!.putImageData(renderer.image, 0, 0)
  return { tick: sim.tick, pixelHash: h.toString(16), png: c.toDataURL('image/png') }
}
function captureAt(tick: number) {
  return new Promise(resolve => captures.set(tick, resolve))
}

let dirty = true
function frame(now: number) {
  acc += Math.min(250, now - last)
  last = now
  if (!paused) {
    while (acc >= TICK_MS) {
      if (!stepOnce()) { acc = 0; break }
      acc -= TICK_MS
    }
    // Joining, or fallen behind: replay quickly, within a frame budget.
    const until = performance.now() + 12
    while (net.behind > 0 && performance.now() < until && stepOnce()) { /* catch up */ }
  } else acc = 0
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
  const others = [...net.roster.keys()].filter(p => p !== net.self && net.roster.get(p)!.until === Infinity)
  const sync = net.desyncAt !== undefined ? `DESYNC at tick ${net.desyncAt}` : `in sync (${net.checks} checks)`
  lines.push(`net ${transport.kind}  me ${net.self}${net.host === net.self ? ' (host)' : ''}  peers ${others.length}  ${net.state}${net.waitingFor ? `  waiting for ${net.waitingFor}` : ''}  ${sync}`)
  if (hover) {
    const [x, y, z] = hover
    lines.push(`cell ${x},${y},${z}  ${mat(world.get(x, y, z)).name}  state ${world.getState(x, y, z)}`)
  }
  hud.textContent = lines.join('\n')
}

// Inspectable from the console (pillar 6).
Object.assign(window, { fv: { sim, world, renderer, spec, net, issue, step: stepOnce, captureAt } })

fit()
requestAnimationFrame(frame)
