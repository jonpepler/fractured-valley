// Speed page: the dead-tree scene in both WebAssembly builds (see
// speed-worker.ts). Like the threads page it needs cross-origin isolation,
// which coi-serviceworker.js arranges, and runs the simulation in a worker.

declare global { interface Window { result?: unknown; error?: string } }

const out = document.getElementById('out')!
const params = new URLSearchParams(location.search)
const threads = Number(params.get('threads') ?? 4)
const ticks = Number(params.get('ticks') ?? 100)

if (!crossOriginIsolated) {
  out.textContent = 'not cross-origin isolated yet: waiting for the service worker to reload the page…'
} else {
  out.textContent = `running the dead tree for ${ticks} ticks, single-threaded and on ${threads} workers…`
  const worker = new Worker(new URL('./speed-worker.js', import.meta.url), { type: 'module' })
  worker.onmessage = ({ data }) => {
    if (data.error) { window.error = data.error; out.textContent = `error: ${data.error}`; return }
    window.result = data
    out.textContent = `single-threaded ${data.st.msPerTick.toFixed(1)} ms/tick, ${data.threads} workers ${data.mt.msPerTick.toFixed(1)} ms/tick`
  }
  worker.onerror = e => { window.error = e.message; out.textContent = `worker error: ${e.message}` }
  worker.postMessage({ threads, ticks })
}
