// Renders the key moments listed in viewer/frames.json as PNGs (deviceScaleFactor 2).
//   cargo run --release --example viewers && node tools/frames.cjs [outdir]
const { chromium } = require('playwright');
const fs = require('fs');
const path = require('path');
(async () => {
  const root = path.resolve(__dirname, '..');
  const out = path.resolve(process.argv[2] || path.join(root, 'out', 'frames'));
  fs.mkdirSync(out, { recursive: true });
  const list = JSON.parse(fs.readFileSync(path.join(root, 'viewer', 'frames.json'), 'utf8'));
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 });
  for (const f of list) {
    const url = 'file://' + path.join(root, 'viewer', f.file) + `?t=${f.t}&h=${f.h}`;
    await page.goto(url);
    await page.waitForFunction(() => window.__ready === true);
    await page.evaluate(label => {
      const d = document.createElement('div');
      d.textContent = label;
      d.style.cssText = 'margin:6px 16px 0;padding:6px 10px;background:#111827;color:#fff;border-radius:6px;font:600 14px system-ui';
      document.querySelector('header').appendChild(d);
    }, f.label);
    await page.screenshot({ path: path.join(out, f.name + '.png'), fullPage: true });
    console.log(f.name + '.png');
  }
  await browser.close();
})();
