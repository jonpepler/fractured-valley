// Shared bits for the browser tests: start the header-less static server,
// load Playwright, read the native hash list.
import { spawn } from 'node:child_process'
import { createRequire } from 'node:module'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const require = createRequire(import.meta.url)
// The local install if there is one (web/node_modules), else a global one.
export const pw = require('playwright')
export const WEB = join(dirname(fileURLToPath(import.meta.url)), '..')
export const native = () => JSON.parse(readFileSync(join(WEB, 'dist/native.json'), 'utf8'))

/** Serve web/dist on `port` with scripts/serve.mjs; resolves to its base URL. */
export async function serve(port) {
  const p = spawn(process.execPath, [join(WEB, 'scripts/serve.mjs'), join(WEB, 'dist'), String(port)], { stdio: ['ignore', 'pipe', 'inherit'] })
  await new Promise((ok, fail) => { p.stdout.once('data', ok); p.once('exit', c => fail(new Error(`server exited ${c}`))) })
  process.on('exit', () => p.kill())
  return { url: `http://127.0.0.1:${port}/`, stop: () => p.kill() }
}

/** Wait for a page to publish `window.result` (or `window.error`). */
export async function result(page, timeout = 300_000) {
  await page.waitForFunction(() => window.result || window.error, null, { timeout, polling: 100 })
  const { result, error } = await page.evaluate(() => ({ result: window.result, error: window.error }))
  if (error) throw new Error(error)
  return result
}

/** First index where two lists differ, or -1. */
export const firstDiff = (a, b) => { for (let i = 0; i < Math.max(a.length, b.length); i++) if (a[i] !== b[i]) return i; return -1 }
