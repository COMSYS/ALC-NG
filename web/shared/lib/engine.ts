import { logErr, logInfo, logLine, logOk, logv } from '../state/log'
import { steps } from '../state/steps'

// --- SwiftLaTeX engine (drives the engine worker directly) ------------------
//
// The official PdfTeXEngine.js wrapper misbehaves here, so the page speaks
// the worker protocol from swiftlatexpdftex.js (pre.js) itself:
//   {cmd: 'writefile', url, src}  -> {result: 'ok'|'failed', cmd}
//   {cmd: 'mkdir', url}           -> {result: 'ok'|'failed', cmd}
//   {cmd: 'setmainfile', url}     -> (no reply)
//   {cmd: 'compilelatex'}         -> {result, status, log, pdf, cmd: 'compile'}
//   {cmd: 'flushcache'}           -> (no reply)
// plus the alcng_* commands of the WASI shim (wasm-test/swiftlatex/alc-wasi.js).
//
// Two engines speak this protocol. XeTeX writes an XDV (handed back in the
// reply's `pdf` field); a separate dvipdfmx worker turns it into the PDF.

export type EngineName = 'pdftex' | 'xetex'

export interface EngineSpec {
  label: string
  worker: string
  fmtName: string
  fmtKey: string
  xdv: boolean
}

export const ENGINES: Record<EngineName, EngineSpec> = {
  pdftex: {
    label: 'pdfTeX',
    worker: '/wasm-test/swiftlatex/swiftlatexpdftex.js',
    fmtName: 'swiftlatexpdftex.fmt',
    fmtKey: 'alc_ng_swiftlatexpdftex_fmt_v1',
    xdv: false,
  },
  xetex: {
    label: 'XeTeX',
    worker: '/wasm-test/swiftlatex/swiftlatexxetex.js',
    fmtName: 'swiftlatexxetex.fmt',
    fmtKey: 'alc_ng_swiftlatexxetex_fmt_v1',
    xdv: true,
  },
}

// Every reply field any command of the engine or dvipdfmx workers sends.
export interface WorkerReply {
  result?: 'ok' | 'failed'
  cmd?: string
  status?: number
  log?: string
  pdf?: ArrayBuffer
  content?: ArrayBuffer
  names?: string[]
  // alcng_list: ListEntry[]; openlog: the engine's access log
  entries?: unknown[]
  data?: Uint8Array
  json?: string
  error?: string
  reloadRequired?: boolean
  removed?: boolean
  exports?: string[]
  level?: string
  text?: string
}

export interface ListEntry {
  path: string
  size: number
}

export const listEntries = (reply: WorkerReply) => (reply.entries ?? []) as ListEntry[]

export interface WorkerMessage {
  cmd: string
  quiet?: boolean
  [field: string]: unknown
}

export class EngineError extends Error {
  engineData?: WorkerReply
}

interface Pending {
  cmds: string[] | null
  handler: (data: WorkerReply) => void
  reject: (err: Error) => void
}

interface TexWorker {
  worker: Worker
  engine: EngineName
  ready: boolean
  pending: Pending | null
  // the format dump seeded into this worker, once provided
  fmtBytes?: Uint8Array
}

let texWorker: TexWorker | null = null

// Whether the alc_ng module is instantiated inside the current engine worker.
export const alcModule = { loaded: false }

// Callbacks run whenever the engine worker is replaced: the compiled PDF and
// the compare results of the old one go with it.
const resetHooks: (() => void)[] = []
export const onEngineReset = (hook: () => void) => resetHooks.push(hook)

export const texReady = () => texWorker !== null && texWorker.ready
export const currentEngine = () => texWorker?.engine ?? null
export const engineSpec = () => ENGINES[texWorker?.engine ?? 'pdftex']
export const engineWorker = () => texWorker

// Serialized call queues: the workers answer one call at a time, and the
// review's lazy file reads (alcng_read) can overlap with the next pipeline
// stage. Both worker protocols share this queue shape.
export interface CallQueue {
  run<T>(thunk: () => Promise<T>): Promise<T>
  reset(): void
}

export function makeCallQueue(): CallQueue {
  let tail: Promise<unknown> = Promise.resolve()
  return {
    run(thunk) {
      const result = tail.then(thunk, thunk)
      tail = result.catch(() => {})
      return result
    },
    reset() {
      tail = Promise.resolve()
    },
  }
}

const texQueue = makeCallQueue()

// Worker console lines → page log, under a per-worker prefix. The kpathsea
// hook's "Start downloading texlive file …" lines additionally drive the
// running step's TeX Live download counter (engine and dvipdfmx alike).
export function forwardWorkerLog(data: WorkerReply, label: string) {
  const text = data.text ?? ''
  if (text.startsWith('Start downloading texlive file')) steps.texDownload()
  logLine(`[${label}] ${text}`, data.level === 'error' ? 'error' : 'verbose')
}

// Spawns the worker of `engine`, replacing the current one. The alc_ng module
// lives inside the engine worker, so a switch also drops it
// (ensureWorkerAlcModule loads it again).
export async function loadEngine(engine: EngineName = 'pdftex') {
  const spec = ENGINES[engine]
  for (const hook of resetHooks) hook()
  dropEngine()

  // No cache-bust query on purpose: the server answers these static files
  // `no-cache` with `Last-Modified`, so every load revalidates and gets a 304
  // unless the worker script actually changed.
  logInfo(`spawning ${spec.label} engine worker ${spec.worker}`)
  const worker = new Worker(spec.worker)
  const mine: TexWorker = { worker, engine, ready: false, pending: null }
  texWorker = mine
  bindWorkerMessages(worker)

  try {
    const t0 = performance.now()
    // The worker's own startup confirms readiness with a spontaneous
    // {result: "ok"} after its runtime initialized.
    await awaitWorkerReply(null, 'engine worker did not initialize')
    worker.postMessage({ cmd: 'settexliveurl', url: '/texlive/' })
    logOk(`${spec.label} engine ready in ${(performance.now() - t0).toFixed(0)} ms`)
    mine.ready = true
  } catch (err) {
    logErr(`engine load failed: ${(err as Error).message}`)
    if (texWorker === mine) dropEngine()
  }
}

// Terminates the engine worker; a call still waiting for it is rejected, so
// nothing hangs on a worker that can no longer answer.
function dropEngine() {
  const current = texWorker
  texWorker = null
  alcModule.loaded = false
  texQueue.reset()
  if (!current) return
  const pending = current.pending
  current.pending = null
  pending?.reject(new Error('engine worker was replaced'))
  try {
    current.worker.terminate()
  } catch {
    // already gone
  }
}

// One persistent onmessage handler: worker console lines go to the log,
// replies are matched against the single pending call.
function bindWorkerMessages(worker: Worker) {
  worker.onmessage = (ev: MessageEvent<WorkerReply>) => {
    const data = ev.data
    if (data?.cmd === 'workerlog') {
      forwardWorkerLog(data, 'engine')
      return
    }
    const current = texWorker
    if (current?.worker !== worker) return
    const pending = current.pending
    if (!pending) {
      logv(`[engine] unhandled message: ${JSON.stringify(data).slice(0, 140)}`)
      return
    }
    const matches = pending.cmds === null || (data?.cmd !== undefined && pending.cmds.includes(data.cmd))
    if (matches) {
      current.pending = null
      pending.handler(data)
    } else {
      logv(`[engine] interjected message while waiting for ${pending.cmds}: ${JSON.stringify(data).slice(0, 120)}`)
    }
  }
  worker.onerror = (err) => {
    logErr(`[engine] worker crashed: ${err.message ?? err}`)
    // The worker cannot answer anymore: reject its call and force the next
    // run to spawn a fresh engine (and load the alc_ng module into it).
    if (texWorker?.worker !== worker) return
    const pending = texWorker.pending
    texWorker.pending = null
    pending?.reject(new Error(`engine worker crashed: ${err.message ?? err}`))
    dropEngine()
  }
}

// Resolves with the worker's next reply to one of `cmds`. `cmds = null` just
// waits for the next reply (used for engine startup).
function awaitWorkerReply(cmds: string[] | null, failTitle: string): Promise<WorkerReply> {
  return new Promise((resolve, reject) => {
    if (!texWorker) {
      reject(new Error('engine not loaded'))
      return
    }
    if (texWorker.pending) {
      reject(new Error('another engine call is pending'))
      return
    }
    texWorker.pending = {
      cmds,
      handler: (data) => {
        if (cmds !== null && data?.result !== 'ok') {
          const err = new EngineError(failTitle)
          err.engineData = data
          reject(err)
        } else {
          resolve(data)
        }
      },
      reject,
    }
  })
}

// Sends a command and resolves with the worker's reply. Commands that do not
// answer ("setmainfile", "flushcache") pass `responseCmds = null` and resolve
// once posted.
export function texCall(message: WorkerMessage, responseCmds: string[] | null): Promise<WorkerReply> {
  return texQueue.run(() => texCallNow(message, responseCmds))
}

function texCallNow(message: WorkerMessage, responseCmds: string[] | null): Promise<WorkerReply> {
  if (!texWorker) return Promise.reject(new Error('engine not loaded'))
  const worker = texWorker.worker
  if (responseCmds === null) {
    worker.postMessage(message)
    return Promise.resolve({})
  }
  logv(`[page] engine cmd: ${message.cmd}${typeof message.url === 'string' ? ` ${message.url}` : ''}`)
  const reply = awaitWorkerReply(responseCmds, `engine command failed: ${message.cmd}`)
  if (!message.quiet) {
    reply.catch((err: Error) => logErr(err.message.length < 300 ? err.message : `${err.message.slice(0, 280)} …`))
  }
  worker.postMessage(message)
  return reply
}

// A file of the engine worker's filesystem (the cleaned tree, the archive),
// read through the alc_ng shim.
export async function cleanedBytes(path: string): Promise<Uint8Array> {
  const reply = await texCall({ cmd: 'alcng_read', path }, ['alcng_read'])
  return new Uint8Array(reply.data ?? [])
}
