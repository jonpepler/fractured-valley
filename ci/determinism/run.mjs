// node run.mjs <url> <outDir> [browsers…]
// Opens the determinism page in each browser and saves its result as JSON.
import { createRequire } from 'node:module'
import { mkdirSync, writeFileSync } from 'node:fs'
import os from 'node:os'
const require = createRequire(import.meta.url)
const pw = require('playwright')
const [url, outDir, ...names] = process.argv.slice(2)
mkdirSync(outDir, { recursive: true })
const tag = `${os.platform()}-${os.arch()}`
let failed = false
for (const name of names.length ? names : ['chromium', 'firefox', 'webkit']) {
  const browser = await pw[name].launch()
  const page = await browser.newPage()
  page.on('pageerror', e => console.error(`${name}: ${e.message}`))
  try {
    await page.goto(url)
    await page.waitForFunction(() => window.result, null, { timeout: 300_000 })
    const result = await page.evaluate(() => window.result)
    writeFileSync(`${outDir}/${tag}-${name}.json`, JSON.stringify({ platform: tag, browser: name, version: browser.version(), ...result }, null, 2))
    console.log(`${tag} ${name} ${browser.version()}: final ${result.hashes.at(-1)} pixels ${result.pixelHash} (${result.msPerTick.toFixed(2)} ms/tick)`)
  } catch (e) {
    console.error(`${tag} ${name}: ${e.message}`)
    failed = true
  }
  await browser.close()
}
process.exit(failed ? 1 : 0)
