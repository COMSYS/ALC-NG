import { resetCompare } from '../state/compare'
import { breadcrumb, logErr, logInfo, logOk } from '../state/log'
import { reviewStore, type PipelineConfig } from '../state/review'
import { steps } from '../state/steps'
import { clearPdfView, setFolderStatus, setSubmissionOpen, showTab, uiStore } from '../state/ui'
import { runCompare } from './compare'
import { cleanCompileArtifacts, compileFolderInEngine, flsFromAccessLog, synthesizeFlsFromLog } from './compile'
import {
  ENGINES,
  alcModule,
  currentEngine,
  loadEngine,
  onEngineReset,
  texCall,
  texReady,
  type EngineError,
  type EngineName,
} from './engine'
import { currentFolder, findMainFiles, type Folder } from './folder'
import { dlNote, fmtKB, plural } from './format'
import { fetchWithProgress } from './net'
import { stripTexComments } from './paths'
import { buildReview } from './review'

// --- full alc-ng pipeline (behaves like the native CLI) ---------------------
//
// Stage the folder in the engine worker's filesystem, compile with SwiftLaTeX
// (the wasm stand-in for latexmk), synthesize the recorder file from the
// access log and write it into the staged input, then run the real pipeline
// (alc_ng_run) in that same worker via the WASI shim
// (wasm-test/swiftlatex/alc-wasi.js). strip_exif needs exiftool and is
// unavailable.

// The alc_ng cdylib built for wasm32-wasip1, served by the dev server from the
// repository's cargo target directory unless VITE_ALC_NG_WASM says otherwise.
const WASM_PATH = import.meta.env.VITE_ALC_NG_WASM ?? '/target/wasm32-wasip1/release/alc_ng.wasm'

// A replaced engine takes its PDF and comparison with it.
onEngineReset(() => {
  clearPdfView()
  resetCompare()
  showTab('review')
})

function currentFlags(): PipelineConfig {
  const f = uiStore.get().flags
  return {
    input: '/work',
    target: '/cleaned',
    keep_bib: f.keepBib,
    clean_classes: f.cleanClasses,
    no_zzrm: f.noZzrm,
    skip_watermark: f.skipWatermark,
    resize_images: f.resizeImages,
    im_size: Number(f.imSize) || 512,
    tar: f.tar ? '/cleaned.tar.gz' : null,
    force: f.force,
    log_level: f.logLevel,
  }
}

// Picks the TeX engine for the submission: the flag, else the compiler a
// top-level 00README names (arXiv's processing instructions), else XeTeX when
// a .tex file loads a package pdfTeX cannot run.
async function chooseEngine(folder: Folder, main: string): Promise<{ engine: EngineName; why: string }> {
  const choice = uiStore.get().flags.engine
  if (choice !== 'auto') return { engine: choice, why: 'selected' }
  for (const [rel, file] of folder.files) {
    if (rel.includes('/') || !/^00readme/i.test(rel)) continue
    const m = /\bcompiler\b\W*([a-z+]+)/i.exec(await file.text())
    if (!m) continue
    if (/^xe/i.test(m[1])) return { engine: 'xetex', why: `${rel} names ${m[1]}` }
    if (/^lua/i.test(m[1])) logErr(`${rel} names ${m[1]}, which has no wasm engine here — trying pdfTeX`)
    return { engine: 'pdftex', why: `${rel} names ${m[1]}` }
  }
  const xetexOnly =
    /\\(?:usepackage|RequirePackage)\s*(?:\[[^\]]*\])?\s*\{[^}]*?\b(fontspec|unicode-math|polyglossia|xeCJK|xltxtra|xunicode|mathspec)\b/
  const texFiles = [main, ...[...folder.files.keys()].filter((rel) => rel !== main && /\.tex$/i.test(rel))]
  for (const rel of texFiles) {
    const m = xetexOnly.exec(stripTexComments(await folder.files.get(rel)!.text()))
    if (m) return { engine: 'xetex', why: `${rel} loads ${m[1]}` }
  }
  return { engine: 'pdftex', why: 'default' }
}

// The multi-MB module bytes, fetched for the engine worker's alcng_load — the
// page never instantiates the module itself; the worker is its only runtime.
async function fetchWasmBytes(): Promise<ArrayBuffer> {
  const t0 = performance.now()
  let bytes: ArrayBuffer
  try {
    bytes = await fetchWithProgress(WASM_PATH, (loaded, total) =>
      steps.detail('cleaner', `Downloading the cleaner, ${dlNote(loaded, total)}`),
    )
  } catch (err) {
    throw new Error(`alc_ng.wasm fetch failed: ${(err as Error).message}`)
  }
  logInfo(`wasm fetch: ${fmtKB(bytes.byteLength)} in ${(performance.now() - t0).toFixed(0)} ms`)
  return bytes
}

async function ensureWorkerAlcModule() {
  if (alcModule.loaded) return
  logInfo('fetching alc_ng.wasm for the engine worker …')
  steps.detail('cleaner', 'Downloading the cleaner')
  const buf = await fetchWasmBytes()
  // from here on nothing streams: the worker spends the actual seconds
  // validating and instantiating the module — say so
  steps.detail('cleaner', 'Starting the cleaner')
  const trace = new URLSearchParams(location.search).has('wasitrace')
  const reply = await texCall({ cmd: 'alcng_load', wasm: buf, trace }, ['alcng_load'])
  alcModule.loaded = true
  if (trace) logInfo('WASI call tracing enabled (?wasitrace=1)')
  logOk(`alc_ng module live in engine worker (${(reply.exports ?? []).length} exports)`)
  steps.detail('cleaner', 'Cleaner ready')
}

export async function runAlcNg() {
  const folder = currentFolder()
  if (!folder || uiStore.get().running) return
  const cfg = currentFlags()
  uiStore.set({ running: true })
  clearPdfView()
  resetCompare()
  setFolderStatus(null)
  steps.begin(folder.label)
  try {
    logInfo(`alc-ng run on "${folder.label}"`)
    const t0 = performance.now()
    steps.start('locate')
    const mains = findMainFiles()
    cfg.user_provided_main_files = mains  // native option: cleans around every checked main
    steps.detail('locate', mains.join(', '))
    steps.start('engine')
    const { engine, why } = await chooseEngine(folder, mains[0])
    logInfo(`engine: ${ENGINES[engine].label} (${why})`)
    if (!texReady() || currentEngine() !== engine) {
      steps.detail('engine', `Starting ${ENGINES[engine].label}`)
      await loadEngine(engine)
    }
    if (!texReady()) throw new Error('engine unavailable')
    steps.detail('engine', `${ENGINES[engine].label}, ${why === 'default' ? 'the default' : why}`)

    const run = await compileFolderInEngine(folder, mains)
    if (run.failed) throw new Error('compile failed — see the processing log')

    let flsText: string
    if (run.inputs.size > 0) {
      flsText = flsFromAccessLog(run)
      logInfo(`recorder file from engine access log: ${run.inputs.size} INPUT, ${run.outputs.size} OUTPUT line(s)`)
    } else {
      logErr('engine access log empty — falling back to compile-log synthesis')
      steps.detail('recorder', 'No file accesses recorded; using the compile log')
      flsText = synthesizeFlsFromLog(run.log)
    }
    // Baseline like native's `latexmk -C`: the artifacts go before the
    // recorder file is written, so the pipeline sees the tree as it stood
    // before compiling.
    steps.detail('recorder', 'Removing compile output')
    await cleanCompileArtifacts(run)
    await texCall({ cmd: 'writefile', url: 'alc-ng-main.fls', src: new TextEncoder().encode(flsText) }, ['writefile'])
    steps.detail('recorder', `${run.inputs.size} ${plural(run.inputs.size, 'input')}, ${run.outputs.size} ${plural(run.outputs.size, 'output')}`)

    steps.start('cleaner')
    await ensureWorkerAlcModule()
    await texCall({ cmd: 'alcng_rm', path: cfg.target }, ['alcng_rm'])
    if (typeof cfg.tar === 'string' && cfg.tar !== '-') await texCall({ cmd: 'alcng_rm', path: cfg.tar }, ['alcng_rm'])

    steps.detail('cleaner', 'Cleaning')
    const reply = await texCall({ cmd: 'alcng_run', json: JSON.stringify(cfg) }, ['alcng_run'])
    const result = JSON.parse(reply.json ?? '{}') as { ok?: boolean; error?: string }
    if (!result.ok) throw new Error(result.error ?? 'the pipeline reported no result')
    steps.start('review')
    await buildReview(folder, cfg, performance.now() - t0, run)
    if (uiStore.get().flags.compare) {
      steps.start('compare')
      const note = uiStore.get().status?.text ?? ''
      const compare = await runCompare(cfg, run, uiStore.get().flags.diffColor)
      setFolderStatus(`${note} PDFs ${compare.state === 'ok' ? 'identical' : compare.state === 'differs' ? compare.text.replace('differing page(s)', 'pages differ') : 'not compared'}.`, compare.state === 'ok' ? 'ok' : 'err')
      steps.done('compare', compare.state === 'ok' ? 'Identical' : compare.state === 'differs' ? `${compare.differing} of ${compare.pages} pages differ` : 'Not completed')
      reviewStore.set((s) => ({ current: s.current && { ...s.current, compare } }))
    } else {
      steps.skip('compare')
    }
    steps.finish()
    // A successful run is the review's moment: fold the controls away and let
    // the changes tree own the rail.
    setSubmissionOpen(false)
  } catch (err) {
    const engineErr = err as EngineError
    const detail = engineErr.engineData?.error ?? engineErr.message ?? String(err)
    logErr(`alc-ng run failed: ${detail}`)
    setFolderStatus(`The run failed: ${detail}`, 'err')
    breadcrumb('RUNERR', detail)
    // A wasm trap leaves the loaded module unusable; the shim says so.
    if (engineErr.engineData?.reloadRequired) alcModule.loaded = false
    steps.fail(detail)
    setSubmissionOpen(true)  // the status alert lives in that panel
  }
  uiStore.set({ running: false })
}
