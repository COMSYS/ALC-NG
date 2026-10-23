import { logErr, logInfo, logOk, logv } from '../state/log'
import type { CompileRun, PipelineConfig } from '../state/review'
import { steps } from '../state/steps'
import { setPdfView } from '../state/ui'
import { xdvToPdf } from './dvipdfm'
import { cleanedBytes, engineSpec, listEntries, texCall, type EngineError, type WorkerReply } from './engine'
import { drainEngineOpenLog, logEngineLog, logEngineTeXLog } from './engineLog'
import { dropFmt, provideFormatDump } from './fmt'
import type { Folder } from './folder'
import { fmtKB, plural } from './format'
import { baseOf, dirOf, failTail, normalizeAccessPath, stemOf } from './paths'
import { stageIntoEngine, type StagedFile } from './stage'

// latexmk's rerun loop: SwiftLaTeX's compilelatex is a single TeX pass plus
// BibTeX, so citations and cross-references would stay unresolved. Rerun
// until the .aux and .bbl stop changing, at most MAX_PASSES times (latexmk's
// max_repeat). Returns the last pass's reply; a failed pass ends the loop.
const MAX_PASSES = 5

async function compileLatexPasses(main: string, what: string): Promise<WorkerReply> {
  const stem = stemOf(main)
  let previous = await auxSnapshot(stem)
  for (let pass = 1; ; pass++) {
    steps.detail('compile', `Pass ${pass} (at most ${MAX_PASSES})`)
    let result: WorkerReply
    try {
      result = await texCall({ cmd: 'compilelatex' }, ['compile'])
    } catch (err) {
      // A failed compile comes back as a rejected call; the worker's reply
      // (status + log) rides along on the error.
      result = (err as EngineError).engineData ?? { result: 'failed', log: '' }
      logErr(`${what}: compilelatex rejected in pass ${pass}: ${(err as Error).message}`)
    }
    if (result.result !== 'ok' || result.pdf === undefined) return result
    const current = await auxSnapshot(stem)
    if (current === previous) {
      logInfo(`${what}: ${pass} pass(es), .aux/.bbl settled`)
      return result
    }
    if (pass >= MAX_PASSES) {
      logErr(`${what}: .aux/.bbl still changing after ${MAX_PASSES} passes — stopping, like latexmk`)
      return result
    }
    logInfo(`${what}: pass ${pass} changed the .aux/.bbl — rerunning`)
    previous = current
  }
}

async function auxSnapshot(stem: string): Promise<string> {
  const parts: string[] = []
  for (const ext of ['aux', 'bbl']) {
    try {
      const file = await texCall({ cmd: 'readfile', url: `${stem}.${ext}`, quiet: true }, ['readfile'])
      parts.push(new TextDecoder().decode(new Uint8Array(file.content ?? new ArrayBuffer(0))))
    } catch {
      parts.push('')  // not written (yet)
    }
  }
  return parts.join('\0')
}

// latexmk -cd: the engine compiles in the main file's directory.
const setEngineMainFile = (rel: string) => texCall({ cmd: 'setmainfile', url: baseOf(rel), dir: dirOf(rel) }, null)

export type CompileResult = ({ failed: false } & CompileRun) | { failed: true; main: string }

// Flushes previous artifacts, stages the folder at /work, compiles every
// checked main file with the SwiftLaTeX engine, shows the PDF, and drains the
// engine's file access log split into submission-relative inputs and outputs.
export async function compileFolderInEngine(folder: Folder, mains: string[]): Promise<CompileResult> {
  steps.start('stage', 'Clearing the previous run')
  await texCall({ cmd: 'flushcache' }, null)  // remove artifacts of previous runs
  logv('[page] /work flushed')
  await provideFormatDump()
  // The format build (when it ran) opened many files; discard its access log
  // so only the actual compile is measured.
  await drainEngineOpenLog()

  steps.detail('stage', `Copying ${folder.files.size} ${plural(folder.files.size, 'file')} into the engine`)
  const staged: StagedFile[] = []  // also handed to dvipdfmx after a XeTeX compile
  for (const rel of [...folder.files.keys()].sort()) {
    // Recorded usage is supplied by the host after compiling; a .fls shipped
    // in the submission belongs to an older build and must not be staged.
    if (/\.fls$/i.test(rel)) continue
    staged.push({ rel, bytes: new Uint8Array(await folder.files.get(rel)!.arrayBuffer()) })
  }
  let written = 0
  await stageIntoEngine(staged, texCall, (rel, bytes) => {
    written += bytes.byteLength
    logv(`[engine] fs write ${rel} (${fmtKB(bytes.byteLength)})`)
  })
  logv(`[page] folder written to engine memfs: ${folder.files.size} file(s), ${fmtKB(written)}`)

  // Every checked main compiles, and the used-file basis unions their access
  // logs (native alc-ng cleans around all of --main-files at once). The first
  // main that compiles is the run's primary: its PDF goes to the pdf tab and
  // the pixel compare, like a single-main run.
  const tCompile = performance.now()
  const inputs = new Set<string>()
  const outputs = new Set<string>()
  let primary: string | null = null
  let pdf: Uint8Array | null = null
  let log = ''
  let primaryLog = ''
  let firstError: { main: string; status: number | string | undefined; log: string } | null = null
  for (const main of mains) {
    const what = mains.length === 1 ? 'compile' : `compile ${main}`
    // Each main gets its own compile step run, so its pass progress and
    // failure details have a running step to attach to.
    steps.start('compile')
    logInfo(`${what} with SwiftLaTeX ${engineSpec().label} (tex packages downloaded from the server on demand)`)
    await setEngineMainFile(main)
    const result = await compileLatexPasses(main, what)
    log = String(result.log ?? '')
    if (result.result !== 'ok' || result.pdf === undefined) {
      const more = mains.length > 1
      logErr(`${what} failed (status ${result.status})${more ? ' — trying the remaining main file(s)' : ''}: ${failTail(log)}`)
      if (!firstError) {
        firstError = { main, status: result.status, log }
        logEngineLog(log)
      }
      steps.detail('compile', `${baseOf(main)} failed${more ? ', trying the next main file' : ''}`)
      continue
    }

    let mainPdf: Uint8Array | null = new Uint8Array(result.pdf)
    logOk(`${what} done: ${engineSpec().xdv ? 'xdv' : 'pdf'} ${fmtKB(mainPdf.length)}, log ${log.length} chars`)
    if (engineSpec().xdv) {
      mainPdf = await xdvToPdf(staged, main, mainPdf)
      if (!mainPdf) {
        logErr(`dvipdfmx failed for ${main}${mains.length > 1 ? ' — trying the remaining main file(s)' : ''}`)
        firstError ??= { main, status: 'dvipdfmx', log }
        continue
      }
    }
    if (!primary) {
      primary = main
      primaryLog = log
    }
    pdf ??= mainPdf

    // The engine's access log is the used-file basis: every open the compile
    // (and bibtex) performed landed in the worker's syscall layer, and
    // `openlog` drains it. Read opens mark inputs, write-only opens outputs.
    steps.start('recorder', main === mains[0] ? 'Reading the file accesses' : `Reading the file accesses of ${baseOf(main)}`)
    const accesses = await drainEngineOpenLog()
    const passInputs = new Set<string>()
    const passOutputs = new Set<string>()
    for (const entry of accesses) {
      const rel = normalizeAccessPath(entry.p, dirOf(main))
      if (rel === null) continue  // absolute path (kpathsea cache etc.)
      if (entry.m === 1) passOutputs.add(rel)  // O_WRONLY
      else passInputs.add(rel)
    }
    // Files the engine both read and wrote across passes (e.g. the .aux) are
    // compile artifacts, not inputs. The .bbl is the exception: BibTeX writes
    // it and the next TeX pass reads it, and latexmk's final pass records it
    // as an INPUT only — native alc-ng keeps those.
    for (const rel of passOutputs) {
      if (/\.bbl$/i.test(rel) && passInputs.has(rel)) passOutputs.delete(rel)
      else passInputs.delete(rel)
    }
    logInfo(`engine access log (${what}): ${accesses.length} open(s) → ${passInputs.size} input(s), ${passOutputs.size} output(s)`)
    steps.detail('recorder', `${passInputs.size} ${plural(passInputs.size, 'input')}, ${passOutputs.size} ${plural(passOutputs.size, 'output')}`)
    for (const rel of passInputs) {
      logv(`  input: ${rel}`)
      inputs.add(rel)
    }
    for (const rel of passOutputs) {
      logv(`  output: ${rel}`)
      outputs.add(rel)
    }
    steps.complete('recorder')
  }

  if (!primary || !pdf) {
    // nothing compiled: report the first failure
    const err = firstError ?? { main: mains[0], status: '?', log: '' }
    if (/format file/.test(err.log)) await dropFmt(engineSpec().fmtKey)
    logErr(`compile of ${err.main} failed (status ${err.status}) — TeX log tail below`)
    await logEngineTeXLog(err.main)
    return { failed: true, main: err.main }
  }
  const compileMs = (performance.now() - tCompile).toFixed(0)
  if (mains.length > 1) logInfo(`compiled ${mains.length} main file(s); the recorder unions their access logs`)
  steps.note('compile', `PDF, ${fmtKB(pdf.length)}`)

  // Show the compiled PDF in the workspace's pdf tab (the primary's).
  setPdfView(pdf)

  return { failed: false, main: primary, mains, compileMs, log: primaryLog || log, inputs, outputs, pdf }
}

// Turns the access log into recorder-file lines. Paths are relative so the
// pipeline resolves them against its own cache folder (helper::parse_fls).
export function flsFromAccessLog(run: Pick<CompileRun, 'inputs' | 'outputs'>): string {
  const lines: string[] = []
  for (const rel of run.inputs) lines.push(`INPUT ./${rel}`)
  for (const rel of run.outputs) lines.push(`OUTPUT ./${rel}`)
  return `${lines.join('\n')}\n`
}

// Synthesizes a recorder file from the engine's compile output, the fallback
// when the access log is empty: pdftex echoes every file it opens as a
// parenthesized name, e.g. `(./main.tex (./used.tex …)`. TeX-distribution
// paths never resolve inside the submission and are ignored by the cleaner.
export function synthesizeFlsFromLog(log: string): string {
  const lines = new Set<string>()
  const paren = /\((\.\/)?([^()\s]+[^\s()])/g
  let match: RegExpExecArray | null
  while ((match = paren.exec(log)) !== null) lines.add(`INPUT ${match[1] ?? './'}${match[2]}`)
  lines.add('INPUT ./main.aux')
  lines.add('OUTPUT ./main.pdf')
  lines.add('OUTPUT ./main.log')
  lines.add('OUTPUT ./main.aux')
  return [...lines].join('\n')
}

// The wasm stand-in for native's `latexmk -C` baseline clean (latexmk_clean in
// submission.rs): remove the files the compile left in /work so the pipeline
// can never count them as used sources. `run.outputs` is exactly that set:
// every file TeX/BibTeX opened for writing, whether it predated the compile or
// not (a .bbl a later pass re-read has already moved into `inputs` and stays,
// as on native). The PDF pane and the compare keep main.pdf from the compile
// response, not from the FS.
export async function cleanCompileArtifacts(run: CompileRun) {
  let doomed = [...run.outputs]
  if (doomed.length === 0) {
    // compile-log fallback (empty access log): at least remove the main
    // file's own artifacts by name.
    const dir = dirOf(run.main) ? `${dirOf(run.main)}/` : ''
    const stem = stemOf(baseOf(run.main))
    doomed = ['aux', 'bbl', 'blg', 'log', 'out', 'toc', 'pdf'].map((ext) => `${dir}${stem}.${ext}`)
  }
  let removed = 0
  for (const rel of doomed) {
    // The engine's cwd is the compile directory, so hand alcng_rm the
    // absolute FS path. Missing files just answer removed:false.
    const reply = await texCall({ cmd: 'alcng_rm', path: `/work/${rel}` }, ['alcng_rm'])
    if (reply.removed) removed += 1
  }
  logInfo(`latexmk -C stand-in: ${removed}/${doomed.length} compile artifact(s) removed from the engine FS`)
}

// Compiles the cleaned tree (read back from the module's /cleaned) with the
// engine; returns the PDF bytes or null when the compile failed.
export async function compileCleanedInEngine(cfg: PipelineConfig, main: string): Promise<Uint8Array | null> {
  const listed = await texCall({ cmd: 'alcng_list', path: cfg.target }, ['alcng_list'])
  const files: StagedFile[] = []
  for (const e of listEntries(listed)) {
    const rel = e.path.startsWith(`${cfg.target}/`) ? e.path.slice(cfg.target.length + 1) : e.path
    files.push({ rel, bytes: await cleanedBytes(e.path) })
  }
  // The tar option renames files with spaces; the loose tree keeps its names.
  const cleanedMain = [main, main.replace(/ /g, '_')].find((m) => files.some((f) => f.rel === m))
  if (!cleanedMain) {
    logErr(`cleaned submission has no main file ${main}`)
    return null
  }

  await texCall({ cmd: 'flushcache' }, null)
  await provideFormatDump()
  await drainEngineOpenLog()
  await stageIntoEngine(files.sort((a, b) => (a.rel < b.rel ? -1 : 1)), texCall)
  logInfo(`compiling cleaned "${cleanedMain}" (${files.length} file(s)) with SwiftLaTeX ${engineSpec().label}`)
  const t0 = performance.now()
  await setEngineMainFile(cleanedMain)
  const result = await compileLatexPasses(cleanedMain, 'cleaned compile')
  if (result.result !== 'ok' || result.pdf === undefined) {
    logErr(`cleaned compile failed (status ${result.status}): ${failTail(result.log)}`)
    logEngineLog(result.log)
    await logEngineTeXLog(cleanedMain)
    return null
  }
  logOk(`cleaned compile done in ${(performance.now() - t0).toFixed(0)} ms`)
  const out = new Uint8Array(result.pdf)
  return engineSpec().xdv ? xdvToPdf(files, cleanedMain, out) : out
}
