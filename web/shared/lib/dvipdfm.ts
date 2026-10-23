import { logErr, logInfo, logOk } from '../state/log'
import { fmtKB } from './format'
import { baseOf, dirOf, stemOf } from './paths'
import { forwardWorkerLog, makeCallQueue, onEngineReset, type CallQueue, type WorkerMessage, type WorkerReply } from './engine'
import { stageIntoEngine, type StagedFile } from './stage'
import { logEngineLog } from './engineLog'

// --- XDV → PDF (dvipdfmx) ------------------------------------------------------
//
// XeTeX hands back an XDV. dvipdfmx runs in its own worker and needs the XDV
// plus everything it references (images) at the same paths under /work;
// fonts come from the /texlive/ proxy like the engine's own requests.

const DVIPDFM_WORKER = '/wasm-test/swiftlatex/swiftlatexdvipdfm.js'

interface Dvi {
  worker: Worker
  queue: CallQueue
  pending: ((data: WorkerReply) => void) | null
  ready: boolean
}

// Spawned on the first xdv → PDF conversion and terminated when the engine
// worker is replaced — its staged memfs files die with it.
let dviPromise: Promise<Dvi> | null = null

function loadDvipdfm(): Promise<Dvi> {
  if (!dviPromise) {
    const promise: Promise<Dvi> = new Promise((resolve, reject) => {
      logInfo(`spawning dvipdfmx worker ${DVIPDFM_WORKER}`)
      const worker = new Worker(DVIPDFM_WORKER)
      const dvi: Dvi = { worker, queue: makeCallQueue(), pending: null, ready: false }
      worker.onmessage = (ev: MessageEvent<WorkerReply>) => {
        const data = ev.data
        if (data?.cmd === 'workerlog') {
          forwardWorkerLog(data, 'dvipdfmx')
          return
        }
        if (!dvi.ready) {
          // the runtime's spontaneous {result: "ok"} after init
          dvi.ready = true
          worker.postMessage({ cmd: 'settexliveurl', url: '/texlive/' })
          resolve(dvi)
          return
        }
        const pending = dvi.pending
        dvi.pending = null
        pending?.(data)
      }
      worker.onerror = (err) => {
        logErr(`[dvipdfmx] worker crashed: ${err.message ?? err}`)
        const pending = dvi.pending
        dvi.pending = null
        dvi.ready = false
        try {
          worker.terminate()
        } catch {
          // already gone
        }
        // Drop this worker's promise so the next conversion spawns a new one;
        // do not clear a newer promise installed by a reset.
        if (dviPromise === promise) dviPromise = null
        if (pending) pending({ result: 'failed', log: String(err.message ?? err) })
        else reject(new Error(`dvipdfmx worker failed: ${err.message ?? err}`))
      }
    })
    dviPromise = promise
    promise.catch(() => {
      if (dviPromise === promise) dviPromise = null
    })
  }
  return dviPromise
}

// Drops the dvipdfmx worker with the engine: terminate once it is up (a load
// still in flight is caught when it resolves), staged memfs files and all.
function resetDvipdfm() {
  const loading = dviPromise
  dviPromise = null
  loading?.then((dvi) => dvi.worker.terminate()).catch(() => {})
}
onEngineReset(resetDvipdfm)

// Serialized like texCall; commands without an answer resolve once posted.
function dviCall(dvi: Dvi, message: WorkerMessage, reply = true): Promise<WorkerReply | undefined> {
  return dvi.queue.run(
    () =>
      new Promise<WorkerReply | undefined>((resolve) => {
        if (reply) dvi.pending = resolve
        dvi.worker.postMessage(message)
        if (!reply) resolve(undefined)
      }),
  )
}

// Returns the PDF bytes, or null when dvipdfmx failed.
export async function xdvToPdf(files: StagedFile[], main: string, xdv: Uint8Array): Promise<Uint8Array | null> {
  const t0 = performance.now()
  const dvi = await loadDvipdfm()
  const xdvRel = `${stemOf(main)}.xdv`
  await dviCall(dvi, { cmd: 'flushcache' }, false)
  await stageIntoEngine([...files, { rel: xdvRel, bytes: xdv }], (msg, cmds) => dviCall(dvi, msg, cmds !== null), null, 'dvipdfmx')
  await dviCall(dvi, { cmd: 'setmainfile', url: baseOf(xdvRel), dir: dirOf(xdvRel) }, false)
  logInfo(`converting "${xdvRel}" to PDF with dvipdfmx`)
  const result = await dviCall(dvi, { cmd: 'compilepdf' })
  if (result?.result !== 'ok' || result.pdf === undefined) {
    logErr(`dvipdfmx failed (status ${result?.status})`)
    logEngineLog(result?.log)
    return null
  }
  const pdf = new Uint8Array(result.pdf)
  logOk(`dvipdfmx done in ${(performance.now() - t0).toFixed(0)} ms: pdf ${fmtKB(pdf.length)}`)
  return pdf
}
