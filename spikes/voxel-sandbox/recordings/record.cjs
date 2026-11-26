// node video2.cjs <scenario.cjs> <outPrefix>
// scenario.cjs exports { ticks, commands: [[tick, kind, x, y, dz?, extra?]], crops: [{ name, at: [x,y,z], w, h, scale, from, to }], wide: bool }
const { chromium } = require('playwright')
const path = require('path')
const { spawn } = require('child_process')
const [,, scenarioPath, out] = process.argv
const sc = require(path.resolve(scenarioPath))
;(async () => {
  const b = await chromium.launch()
  const p = await b.newPage({ viewport: { width: 900, height: 700 } })
  p.on('pageerror', e => console.log('pageerror: ' + e.message))
  await p.goto('http://127.0.0.1:5173/?net=solo' + (sc.bodyStyle ? '&bodies=' + sc.bodyStyle : '') + (sc.view ? '&view=' + sc.view : ''))
  await p.waitForFunction(() => window.fv && fv.sim)
  await p.keyboard.press(' ')
  // Optional setup: boxes of material placed before the first tick ([x0,y0,z0,x1,y1,z1,material,moisture], inclusive).
  if (sc.build) await p.evaluate(boxes => {
    const w = fv.world
    for (const [x0, y0, z0, x1, y1, z1, m, wet] of boxes)
      for (let z = z0; z <= z1; z++) for (let y = y0; y <= y1; y++) for (let x = x0; x <= x1; x++)
        if (w.get(x, y, z) === 0) w.set(x, y, z, m, 0, wet)
  }, sc.build)
  // Optional: set the moisture of every non-air cell in boxes ([x0,y0,z0,x1,y1,z1,moisture]), e.g. a dead, dry tree.
  if (sc.dry) await p.evaluate(boxes => {
    const w = fv.world
    for (const [x0, y0, z0, x1, y1, z1, wet] of boxes)
      for (let z = z0; z <= z1; z++) for (let y = y0; y <= y1; y++) for (let x = x0; x <= x1; x++) {
        const m = w.get(x, y, z)
        if (m === 7 || m === 8) w.set(x, y, z, m, 0, wet)
      }
  }, sc.dry)
  const size = await p.evaluate(cmds => {
    const { sim, world } = fv
    const ground = (x, y) => { let z = world.sz - 1; while (z > 0 && world.get(x, y, z) === 0) z--; return z }
    let seq = 0
    for (const [tick, kind, x, y, dz = 0, extra] of cmds)
      sim.schedule({ tick, peer: 0, seq: seq++, command: { kind, x, y, z: ground(x, y) + dz, ...(extra || {}) } })
    return { w: fv.renderer.width, h: fv.renderer.height }
  }, sc.commands)
  const enc = ['-c:v', 'libx264', '-crf', '16', '-preset', 'slow', '-pix_fmt', 'yuv420p', '-movflags', '+faststart']
  const parts = [], maps = []
  const n = sc.crops.length + (sc.wide ? 1 : 0)
  parts.push(`[0]split=${n}${Array.from({ length: n }, (_, i) => `[s${i}]`).join('')}`)
  let i = 0
  if (sc.wide) { parts.push(`[s${i++}]scale=iw*2:ih*2:flags=neighbor[wide]`); maps.push('-map', '[wide]', ...enc, `${out}-wide.mp4`) }
  for (const c of sc.crops) {
    const [ax, ay] = await p.evaluate(([x, y, z]) => fv.renderer.anchor(x, y, z), c.at)
    const cx = Math.max(0, Math.min(size.w - c.w, Math.round(ax - c.w / 2))), cy = Math.max(0, Math.min(size.h - c.h, Math.round(ay - c.h / 2)))
    const trim = c.from !== undefined ? `trim=start=${c.from}:end=${c.to},setpts=PTS-STARTPTS,` : ''
    parts.push(`[s${i++}]${trim}crop=${c.w}:${c.h}:${cx}:${cy},scale=iw*${c.scale}:ih*${c.scale}:flags=neighbor[${c.name}]`)
    maps.push('-map', `[${c.name}]`, ...enc, `${out}-${c.name}.mp4`)
  }
  const ff = spawn('ffmpeg', ['-y', '-loglevel', 'error', '-f', 'rawvideo', '-pix_fmt', 'rgba', '-s', `${size.w}x${size.h}`, '-r', '20', '-i', '-', '-filter_complex', parts.join(';'), ...maps], { stdio: ['pipe', 'inherit', 'inherit'] })
  const done = new Promise(r => ff.on('close', r))
  const write = buf => new Promise(r => ff.stdin.write(buf) ? r() : ff.stdin.once('drain', r))
  let maxBodies = 0
  for (let t = 0; t < sc.ticks; t += 10) {
    const chunk = await p.evaluate(n => {
      const frames = []
      let mb = 0
      for (let i = 0; i < n; i++) {
        fv.step()
        mb = Math.max(mb, fv.sim.bodies.length)
        fv.renderer.render(fv.sim.grids, fv.sim.tick, fv.sim)
        const bytes = new Uint8Array(fv.renderer.image.data.buffer.slice(0))
        let s = ''
        for (let j = 0; j < bytes.length; j += 0x8000) s += String.fromCharCode.apply(null, bytes.subarray(j, j + 0x8000))
        frames.push(btoa(s))
      }
      return { frames, mb }
    }, Math.min(10, sc.ticks - t))
    maxBodies = Math.max(maxBodies, chunk.mb)
    for (const f of chunk.frames) await write(Buffer.from(f, 'base64'))
  }
  ff.stdin.end(); await done
  const ev = await p.evaluate(() => fv.sim.events.filter(e => e.kind === 'detached' || e.kind === 'landed').map(e => `${e.tick}:${e.kind}:${e.cells}@${e.at}`).join(' '))
  console.log('max bodies', maxBodies); console.log(ev)
  await b.close()
})()
