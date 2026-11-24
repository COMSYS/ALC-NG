import { StrictMode, type ReactNode } from 'react'
import { createRoot } from 'react-dom/client'
import { loadEngine } from './lib/engine'
import { loadSampleFolder } from './lib/folder'
import './lib/pipeline'
import { SAMPLES } from './lib/samples'

// Mounts a design's app (tool/, site/) and applies the query parameters
// both designs share.
export function start(app: ReactNode) {
  createRoot(document.getElementById('root')!).render(<StrictMode>{app}</StrictMode>)

  const params = new URLSearchParams(location.search)
  // ?sample=<index> loads a bundled sample straight away: a reproducible
  // starting point, which also makes the page scriptable from a headless browser.
  const sample = params.get('sample')
  if (sample !== null && SAMPLES[Number(sample)]) void loadSampleFolder(Number(sample))
  // Lazy boot: a first load never fetches more than the page itself — a run
  // cold-starts the engine worker and pulls the wasm module into it on demand,
  // which keeps a first mobile visit page-sized instead of ~11 MB. `?preload=1`
  // warms the engine worker at load for sessions that measure compile times.
  if (params.has('preload')) void loadEngine()
}
