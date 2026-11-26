// Threads page. Shared memory needs the page to be cross-origin isolated;
// coi-serviceworker.js (loaded first by the HTML) arranges that on servers
// that cannot send the headers, such as GitHub Pages. The simulation itself
// runs in a Web Worker, because rayon's calling thread has to be able to
// block while it waits for the pool, and a page's main thread cannot.

declare global { interface Window { result?: unknown; error?: string } }

const out = document.getElementById('out')!
const params = new URLSearchParams(location.search)
const threads = Number(params.get('threads') ?? 4)
const nav = performance.getEntriesByType('navigation')[0] as PerformanceNavigationTiming | undefined

if (!crossOriginIsolated) {
  // coi-serviceworker reloads the page once its worker is in control.
  out.textContent = 'not cross-origin isolated yet: waiting for the service worker to reload the page…'
} else {
  out.textContent = `cross-origin isolated; running with ${threads} workers…`
  const worker = new Worker(new URL('./threads-worker.js', import.meta.url), { type: 'module' })
  worker.onmessage = ({ data }) => {
    if (data.error) { window.error = data.error; out.textContent = `error: ${data.error}`; return }
    window.result = { ...data, crossOriginIsolated, navigationType: nav?.type, serviceWorker: !!navigator.serviceWorker?.controller }
    out.textContent = JSON.stringify(window.result, null, 2)
  }
  worker.onerror = e => { window.error = e.message; out.textContent = `worker error: ${e.message}` }
  worker.postMessage({ threads })
}
