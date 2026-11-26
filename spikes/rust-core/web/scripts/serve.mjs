// node scripts/serve.mjs <dir> <port>
// A plain static file server that sends NO cross-origin isolation headers
// (no COOP, no COEP), like GitHub Pages. The threads page has to get them
// from the service worker instead.
import { createServer } from 'node:http'
import { readFile, stat } from 'node:fs/promises'
import { extname, join, normalize, resolve } from 'node:path'

const [dir = 'dist', port = '4180'] = process.argv.slice(2)
const root = resolve(dir)
const types = { '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.wasm': 'application/wasm', '.json': 'application/json', '.css': 'text/css', '.ts': 'text/plain' }

createServer(async (req, res) => {
  const path = decodeURIComponent(new URL(req.url, 'http://x').pathname)
  let file = normalize(join(root, path))
  if (!file.startsWith(root)) { res.writeHead(403).end(); return }
  try {
    if ((await stat(file)).isDirectory()) file = join(file, 'index.html')
    const body = await readFile(file)
    res.writeHead(200, { 'Content-Type': types[extname(file)] ?? 'application/octet-stream', 'Cache-Control': 'no-store' })
    res.end(body)
  } catch {
    res.writeHead(404).end('not found')
  }
}).listen(Number(port), '127.0.0.1', () => console.log(`serving ${root} on http://127.0.0.1:${port}/ (no COOP/COEP headers)`))
