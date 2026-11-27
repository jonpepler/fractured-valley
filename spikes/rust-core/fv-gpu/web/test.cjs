// Headless Chromium with WebGPU runs fv-gpu/web against the dumped phase.
//   cargo run --release -p fv-gpu --example dump_phase -- fv-gpu/web/data
//   (cd fv-gpu/web && python3 -m http.server 8766 &) ; node fv-gpu/web/test.cjs
const { chromium } = require('playwright');
(async () => {
  const b = await chromium.launch({ channel: 'chromium', headless: true, args: ['--enable-unsafe-webgpu', '--enable-features=Vulkan', '--use-vulkan=native', '--use-angle=vulkan'] });
  const p = await b.newPage();
  p.on('console', (m) => console.log('page:', m.text()));
  await p.goto(process.env.URL || 'http://localhost:8766/index.html');
  await p.waitForFunction(() => window.result, null, { timeout: 600000 });
  const r = await p.evaluate(() => window.result);
  console.log(JSON.stringify(r, null, 1));
  await b.close();
  process.exit(r.error || r.wordsDiffering ? 1 : 0);
})();
