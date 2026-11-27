// The diffusion phase's WGSL in the browser: the same inputs as native,
// compared word for word with the CPU's result. Sets window.result.
const get = async (f) => new Uint32Array(await (await fetch(f)).arrayBuffer());
const ms = (t0) => performance.now() - t0;

async function main() {
  const adapter = await navigator.gpu.requestAdapter({ powerPreference: 'high-performance' });
  const info = adapter.info || {};
  const device = await adapter.requestDevice({ requiredLimits: {
    maxStorageBufferBindingSize: adapter.limits.maxStorageBufferBindingSize,
    maxBufferSize: adapter.limits.maxBufferSize } });
  const src = await (await fetch('data/diffuse.wgsl')).text();
  const [params, cells, sides, vel, expected] = await Promise.all(['params', 'cells', 'sides', 'vel', 'expected'].map((n) => get(`data/${n}.bin`)));
  const n = params[5], nlower = params[9], bricks = params[12];
  const module = device.createShaderModule({ code: src });
  const msgs = (await module.getCompilationInfo()).messages.filter((m) => m.type === 'error');
  if (msgs.length) throw new Error(msgs.map((m) => `${m.lineNum}: ${m.message}`).join('\n'));
  const U = GPUBufferUsage;
  const buf = (words, usage) => device.createBuffer({ size: words * 4, usage });
  const B = {
    params: buf(16, U.UNIFORM | U.COPY_DST), cells: buf(cells.length, U.STORAGE | U.COPY_DST),
    sides: buf(sides.length, U.STORAGE | U.COPY_DST), vel: buf(vel.length, U.STORAGE | U.COPY_DST),
    faces: buf((3 * n + nlower) * 21, U.STORAGE), limits: buf(2 * n, U.STORAGE),
    out: buf(n * 20, U.STORAGE | U.COPY_SRC), edge: buf((n + bricks) * 16, U.STORAGE | U.COPY_SRC),
  };
  const read = buf(n * 20 + bricks * 16, U.MAP_READ | U.COPY_DST);
  const all = [B.params, B.cells, B.sides, B.vel, B.faces, B.limits, B.out, B.edge];
  const names = ['faces_main', 'limits_main', 'brick_main', 'tally_main'];
  const uses = [[0, 1, 2, 3, 4], [0, 1, 4, 5], [0, 1, 2, 4, 5, 6, 7], [0, 7]];
  const threads = [3 * n + nlower, n, n, bricks];
  const pipes = await Promise.all(names.map((e) => device.createComputePipelineAsync({ layout: 'auto', compute: { module, entryPoint: e } })));
  const groups = pipes.map((p, k) => device.createBindGroup({ layout: p.getBindGroupLayout(0), entries: uses[k].map((b) => ({ binding: b, resource: { buffer: all[b] } })) }));
  const dispatch = (pass, t) => { const g = Math.ceil(t / 64); if (g <= 65535) pass.dispatchWorkgroups(g); else pass.dispatchWorkgroups(65535, Math.ceil(g / 65535)); };

  const once = async () => {
    const t = {};
    let t0 = performance.now();
    device.queue.writeBuffer(B.params, 0, params); device.queue.writeBuffer(B.cells, 0, cells);
    device.queue.writeBuffer(B.sides, 0, sides); device.queue.writeBuffer(B.vel, 0, vel);
    await device.queue.onSubmittedWorkDone(); t.upload = ms(t0);
    t0 = performance.now();
    const enc = device.createCommandEncoder();
    const pass = enc.beginComputePass();
    for (let k = 0; k < 4; k++) { pass.setPipeline(pipes[k]); pass.setBindGroup(0, groups[k]); dispatch(pass, threads[k]); }
    pass.end();
    device.queue.submit([enc.finish()]);
    await device.queue.onSubmittedWorkDone(); t.compute = ms(t0);
    t0 = performance.now();
    const e2 = device.createCommandEncoder();
    e2.copyBufferToBuffer(B.out, 0, read, 0, n * 80);
    e2.copyBufferToBuffer(B.edge, n * 64, read, n * 80, bricks * 64);
    device.queue.submit([e2.finish()]);
    await read.mapAsync(GPUMapMode.READ);
    const got = new Uint32Array(read.getMappedRange()).slice();
    read.unmap(); t.readback = ms(t0);
    return { t, got };
  };
  // A tiny round trip: what readback latency costs with nothing to copy.
  const small = buf(4, U.MAP_READ | U.COPY_DST);
  const tiny = [];
  for (let i = 0; i < 10; i++) {
    const t0 = performance.now();
    const e = device.createCommandEncoder(); e.copyBufferToBuffer(B.out, 0, small, 0, 16); device.queue.submit([e.finish()]);
    await small.mapAsync(GPUMapMode.READ); small.unmap(); tiny.push(ms(t0));
  }
  let first = await once();
  let diff = 0, firstDiff = -1;
  for (let i = 0; i < expected.length; i++) if (first.got[i] !== expected[i]) { diff++; if (firstDiff < 0) firstDiff = i; }
  const runs = [];
  const reps = Number(new URLSearchParams(location.search).get('reps') || 8);
  for (let r = 0; r < reps; r++) runs.push((await once()).t);
  const best = (k) => Math.min(...runs.map((t) => t[k]));
  return {
    adapter: `${info.isFallbackAdapter ? '(fallback) ' : ''}${info.vendor || '?'} ${info.architecture || ''} ${info.device || ''} ${info.description || ''}`.trim(),
    cells: n, words: expected.length, wordsDiffering: diff, firstDiff,
    bestMs: { upload: best('upload'), compute: best('compute'), readback: best('readback'), total: Math.min(...runs.map((t) => t.upload + t.compute + t.readback)) },
    tinyReadbackMs: Math.min(...tiny),
  };
}
main().then((r) => { window.result = r; document.getElementById('out').textContent = JSON.stringify(r, null, 1); })
  .catch((e) => { window.result = { error: String(e && e.stack || e) }; document.getElementById('out').textContent = String(e); });
