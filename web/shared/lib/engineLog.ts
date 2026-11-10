import { logErr, logv } from '../state/log'
import { texCall, type EngineError } from './engine'
import { stemOf } from './paths'

// A failed compile is otherwise undiagnosable: the worker only hands back the
// tail of pdfTeX's output. Echo the last lines of the engine log so the
// processing log shows what TeX actually complained about.
export function logEngineLog(log: unknown) {
  const text = String(log ?? '').replace(/\r/g, '\n')
  const lines = text.split('\n').filter((l) => l.trim().length > 0)
  for (const line of lines.slice(-30)) logErr(`[engine] ${line.trim()}`)
}

// The drained access log records opens, not TeX errors, and pdfTeX writes
// its report to <main>.log in the engine's memfs. Read that file back
// through the module's file reader and echo the tail.
export async function logEngineTeXLog(main: string) {
  const stem = stemOf(main || 'main')
  for (const path of [`/work/${stem}.log`, '/work/main.log']) {
    try {
      const reply = await texCall({ cmd: 'alcng_read', path, quiet: true }, ['alcng_read'])
      const lines = new TextDecoder()
        .decode(new Uint8Array(reply.data ?? []))
        .split(/\r?\n/)
        .filter((l) => l.trim().length > 0)
      if (lines.length === 0) continue
      logErr(`[engine] ${path} — last ${Math.min(40, lines.length)} of ${lines.length} line(s):`)
      for (const line of lines.slice(-40)) logErr(`[engine] ${line.trim()}`)
      return
    } catch (err) {
      logv(`[page] no engine log at ${path}: ${(err as Error).message}`)
    }
  }
}

// Debug helper: dumps the tail of the engine log and /work contents into the
// tab title, which always shows up in test snapshots.
export async function logWorkDirBreadcrumb(err: EngineError, label: string) {
  let listing: string
  try {
    const d = await texCall({ cmd: 'listdir' }, ['listdir'])
    listing = (d.names ?? []).join(',')
  } catch (e) {
    listing = `listdir failed: ${String((e as Error).message).slice(0, 60)}`
  }
  const log = err?.engineData?.log ?? ''
  const tail = log.slice(-200).replace(/\n/g, ' ¦ ')
  logErr(`[engine] /work = [${listing}] — log end: ${tail}`)
  document.title = `${label} work=[${listing}] end[${tail}]`
}

export interface AccessEntry {
  p: string
  m: number
}

// Reads (and clears) the engine worker's file access log. Every fopen the
// engine performed — inputs by pdftex/bibtex as well as outputs — was
// recorded in its syscall layer.
export async function drainEngineOpenLog(): Promise<AccessEntry[]> {
  const reply = await texCall({ cmd: 'openlog' }, ['openlog'])
  return (reply.entries ?? []) as AccessEntry[]
}
