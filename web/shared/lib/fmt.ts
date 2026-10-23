import { logErr, logInfo, logOk, logv } from '../state/log'
import { steps } from '../state/steps'
import { engineSpec, engineWorker, texCall, type EngineError, type EngineSpec } from './engine'
import { logWorkDirBreadcrumb } from './engineLog'
import { dlNote, fmtKB } from './format'
import { fetchWithProgress } from './net'

// --- the format dump ---------------------------------------------------------
//
// The engine cannot load a format dump from disk (SwiftLaTeX's mirror is
// offline), so on first use it builds the LaTeX format itself from the local
// TeX Live (`compileformat`, i.e. `*pdflatex.ini` / `*xelatex.ini`) and the
// result is seeded back into its kpathsea hook under the name the engine
// looks up.
//
// Two caches, cheapest first: this worker's memory, IndexedDB (this browser),
// then the server's `wasm-test/fmt-cache/`. The server copy is baked into the
// image at build time (wasm-test/build-fmt.cjs) and served read-only. A dump
// is only valid for the engine build that wrote it, so the URL carries the
// versioned `fmtKey`; if the served dump fails to load, this session builds
// its own.

const badServerFmts = new Set<string>()  // per key: a served dump failed to load

const fmtServerUrl = (spec: EngineSpec) =>
  `/fmt/${encodeURIComponent(spec.fmtKey)}/${encodeURIComponent(spec.fmtName)}`

async function fetchServerFmt(spec: EngineSpec): Promise<Uint8Array | null> {
  if (badServerFmts.has(spec.fmtKey)) return null
  try {
    // the dump is 17–28 MB — show the percentage on the stage step
    const buf = await fetchWithProgress(fmtServerUrl(spec), (loaded, total) =>
      steps.detail('stage', `Downloading the ${spec.label} format, ${dlNote(loaded, total)}`),
    )
    return buf.byteLength >= 1024 ? new Uint8Array(buf) : null  // a real dump is megabytes
  } catch {
    return null  // 404, or no server: build locally
  }
}

export async function provideFormatDump(): Promise<Uint8Array> {
  const worker = engineWorker()
  if (!worker) throw new Error('engine not loaded')
  if (worker.fmtBytes !== undefined) {
    logv('format dump already seeded for this session')
    return worker.fmtBytes
  }
  const spec = engineSpec()

  // One dump from one source: remember it for the session, cache it and seed
  // the engine's kpathsea hook. `note` is the log line naming the source.
  async function adopt(dump: Uint8Array, note: string, { level = 'ok', cache = true } = {}) {
    worker!.fmtBytes = dump
    if (cache) await saveFmt(spec.fmtKey, dump)
    ;(level === 'ok' ? logOk : logInfo)(note)
    await seedFormatDump(spec, dump)
    return dump
  }

  // Reloads reuse the previously built dump.
  const stored = await loadFmt(spec.fmtKey)
  if (stored) {
    steps.detail('stage', 'Format loaded from the browser cache')
    return adopt(stored, `restored ${spec.label} format dump from IndexedDB (${fmtKB(stored.length)}) — skipping the build`, {
      level: 'info',
      cache: false,
    })
  }

  // A browser that has never built one asks the server first — this is what
  // saves the multi-minute INITEX build on a fresh profile.
  steps.detail('stage', 'Looking for a prebuilt format')
  const remote = await fetchServerFmt(spec)
  if (remote) {
    steps.detail('stage', 'Prebuilt format downloaded')
    return adopt(remote, `restored ${spec.label} format dump from the server cache (${fmtKB(remote.length)}) — skipping the build`)
  }

  logInfo(
    `building the ${spec.label} LaTeX format dump with the engine (once, ~a few minutes — INITEX loads the full LaTeX2e kernel + hyphenation patterns)`,
  )
  steps.detail('stage', `Building the ${spec.label} format; this takes a few minutes once`)
  try {
    const result = await texCall({ cmd: 'compileformat' }, ['compile'])
    if (result.result === 'ok' && result.pdf !== undefined) {
      const dump = new Uint8Array(result.pdf)
      await adopt(dump, `format dump built: ${fmtKB(dump.length)} (cached in IndexedDB for later runs)`)
      logv("format dump seeded into the engine's kpathsea hook")
      return dump
    }
    // A reply without a dump must fail here — silently returning would let
    // the run continue against an unseeded format.
    throw new Error(`compileformat answered ${JSON.stringify(result?.result ?? result)} without a dump`)
  } catch (err) {
    const engineErr = err as EngineError
    // The engine names its dump after the job's input path (the kpathsea
    // cache id of the .ini), so its own readback misses (-253 = "fetch
    // content failed"). Recover by grabbing the freshly written *.fmt from
    // /work directly.
    if (engineErr.engineData?.status === -253) {
      logv('official readback missed (dump named after the job\'s cache id) — grabbing the fresh *.fmt from /work')
      const dump = await grabBuiltFormatDump()
      if (dump) return adopt(dump, `format dump recovered from /work: ${fmtKB(dump.length)} (cached in IndexedDB for later runs)`)
      logErr("no *.fmt found in the engine's /work")
    } else {
      logErr(`format build failed: ${engineErr.message.slice(0, 200)}`)
    }
    await logWorkDirBreadcrumb(engineErr, 'LOGBUILD')
    throw err
  }
}

async function grabBuiltFormatDump(): Promise<Uint8Array | null> {
  const listing = await texCall({ cmd: 'listdir' }, ['listdir'])
  const fmtName = (listing.names ?? []).find((n) => n.toLowerCase().endsWith('.fmt'))
  if (!fmtName) return null
  const file = await texCall({ cmd: 'readfile', url: fmtName }, ['readfile'])
  return file.content !== undefined ? new Uint8Array(file.content) : null
}

async function seedFormatDump(spec: EngineSpec, fmtBytes: Uint8Array) {
  await texCall({ cmd: 'seedfiles', files: { [spec.fmtName]: fmtBytes } }, ['seedfiles'])
}

// Format dumps are several MB (XeLaTeX's ~27 MB) — too big for localStorage,
// so they live in IndexedDB, keyed by engine.
function fmtStore<T>(mode: IDBTransactionMode, request: (store: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    const open = indexedDB.open('alc-ng-wasm-test', 1)
    open.onupgradeneeded = () => open.result.createObjectStore('fmt')
    open.onerror = () => reject(open.error)
    open.onsuccess = () => {
      const db = open.result
      const tx = db.transaction('fmt', mode)
      const req = request(tx.objectStore('fmt'))
      tx.oncomplete = () => {
        db.close()
        resolve(req.result)
      }
      tx.onerror = tx.onabort = () => {
        db.close()
        reject(tx.error)
      }
    }
  })
}

async function saveFmt(key: string, bytes: Uint8Array) {
  try {
    await fmtStore('readwrite', (store) => store.put(bytes, key))
  } catch (err) {
    logv(`[page] could not cache the format dump: ${(err as Error)?.message ?? err}`)  // rebuild next time
  }
}

async function loadFmt(key: string): Promise<Uint8Array | null> {
  try {
    const bytes = await fmtStore<unknown>('readonly', (store) => store.get(key))
    return bytes instanceof Uint8Array ? bytes : null
  } catch {
    return null
  }
}

// The dump on hand is unusable (a "format file" error), so distrust the
// server copy too: the rebuild replaces both with a working dump. The
// in-memory copy goes as well, or the next run would reuse it at once.
export async function dropFmt(key: string) {
  badServerFmts.add(key)
  const worker = engineWorker()
  if (worker) worker.fmtBytes = undefined
  try {
    await fmtStore('readwrite', (store) => store.delete(key))
  } catch {
    // nothing cached
  }
}
