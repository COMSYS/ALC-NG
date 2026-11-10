import { createStore } from '../lib/store'

export type LogLevel = 'verbose' | 'info' | 'ok' | 'error'

export interface LogEntry {
  id: number
  time: string
  text: string
  level: LogLevel
}

const LOG_MAX_LINES = 1000

export const logStore = createStore<{ lines: LogEntry[] }>({ lines: [] })

// Lines are collected synchronously and published once per frame: a staging
// pass or a TeX Live burst logs hundreds of lines in a row, and one render per
// line would stall the page.
let pending: LogEntry[] = []
let flushQueued = false
let nextId = 1

function flush() {
  flushQueued = false
  if (pending.length === 0) return
  const lines = [...logStore.get().lines, ...pending]
  pending = []
  logStore.set({ lines: lines.length > LOG_MAX_LINES ? lines.slice(-LOG_MAX_LINES) : lines })
}

const schedule =
  typeof requestAnimationFrame === 'function' ? requestAnimationFrame : (fn: () => void) => setTimeout(fn, 16)

const timestamp = (t: Date) =>
  `${[t.getHours(), t.getMinutes(), t.getSeconds()].map((v) => String(v).padStart(2, '0')).join(':')}` +
  `.${String(t.getMilliseconds()).padStart(3, '0')}`

export function logLine(text: string, level: LogLevel = 'verbose') {
  pending.push({ id: nextId++, time: timestamp(new Date()), text, level })
  if (!flushQueued) {
    flushQueued = true
    schedule(flush)
  }
  if (level === 'error' || level === 'info') {
    logTail.push(text)
    while (logTail.length > 12) logTail.shift()
  }
}

export const logInfo = (text: string) => logLine(text, 'info')
export const logv = (text: string) => logLine(text, 'verbose')
export const logOk = (text: string) => logLine(text, 'ok')
export const logErr = (text: string) => logLine(text, 'error')

export function clearLog() {
  pending = []
  logStore.set({ lines: [] })
  logInfo('log cleared')
}

// The processing log is invisible in headless snapshots; surfacing the
// interesting tail in the tab title makes failures diagnosable from there.
const logTail: string[] = []

export function breadcrumb(label: string, detail: string) {
  document.title = `${label}: ${[detail, ...logTail].join(' | ').slice(0, 480)}`
}
