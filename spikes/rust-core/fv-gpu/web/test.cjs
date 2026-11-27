// Headless Chromium with WebGPU runs fv-gpu/web against the dumped phase,
// served by this script. From spikes/rust-core:
//   cargo run --release -p fv-gpu --example dump_phase -- fv-gpu/web/data
//   NODE_PATH=web/node_modules node fv-gpu/web/test.cjs
// Exits non-zero unless every output word equals the CPU's.
const { chromium } = require('playwright');
const http = require('http');
const fs = require('fs');
const path = require('path');
const types = { '.html': 'text/html', '.js': 'text/javascript', '.wgsl': 'text/plain', '.bin': 'application/octet-stream' };
(async () => {
  const root = __dirname;
  const server = http.createServer((req, res) => {
    const f = path.join(root, decodeURIComponent(new URL(req.url, 'http://x').pathname));
    if (!f.startsWith(root) || !fs.existsSync(f) || fs.statSync(f).isDirectory()) { res.writeHead(404); return res.end(); }
    res.writeHead(200, { 'content-type': types[path.extname(f)] || 'application/octet-stream' });
    fs.createReadStream(f).pipe(res);
  }).listen(0);
  const port = server.address().port;
  const b = await chromium.launch({ channel: 'chromium', headless: true, args: ['--enable-unsafe-webgpu', '--enable-features=Vulkan', '--use-vulkan=native', '--use-angle=vulkan'] });
  const p = await b.newPage();
  p.on('console', (m) => console.log('page:', m.text()));
  await p.goto(`http://localhost:${port}/index.html${process.env.QUERY || ''}`);
  await p.waitForFunction(() => window.result, null, { timeout: 600000 });
  const r = await p.evaluate(() => window.result);
  console.log(JSON.stringify(r, null, 1));
  await b.close();
  server.close();
  process.exit(r.error || r.wordsDiffering ? 1 : 0);
})();
