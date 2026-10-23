import { createStore } from '../lib/store'
import { reviewStore } from './review'
import { showTab } from './ui'

// --- run progress: the pipeline as a proof sheet of steps -------------------
//
// While a run is in flight, the stages report here and the stepper shows them
// in the review pane, in place of the diff that will follow. Every method is
// a no-op outside a run, so deep helpers (compileLatexPasses, the format-dump
// build, the compare loop) may call in unconditionally; detail updates only
// land on the step that is currently running. A finished run fades the sheet
// out; a failed one stays, marking the step where the pipeline stopped.

export const RUN_STEPS = [
  ['locate', 'main file'],
  ['engine', 'engine'],
  ['stage', 'stage folder'],
  ['compile', 'compile'],
  ['recorder', 'recorder file'],
  ['cleaner', 'clean pipeline'],
  ['review', 'build review'],
  ['compare', 'pixel compare'],
] as const

export type StepKey = (typeof RUN_STEPS)[number][0]
export type StepState = 'pending' | 'running' | 'done' | 'failed' | 'skipped'

export interface StepRow {
  state: StepState
  t0: number
  // the step's duration once settled; null while pending or running
  ms: number | null
  sub: string
  // TeX Live files downloaded while the step ran
  downloads: number
}

export interface RunState {
  visible: boolean
  leaving: boolean
  label: string
  t0: number
  current: StepKey | null
  rows: Record<StepKey, StepRow>
  failure: string | null
}

const freshRows = () =>
  Object.fromEntries(
    RUN_STEPS.map(([key]) => [key, { state: 'pending', t0: 0, ms: null, sub: '', downloads: 0 } satisfies StepRow]),
  ) as Record<StepKey, StepRow>

export const runStore = createStore<RunState>({
  visible: false,
  leaving: false,
  label: '',
  t0: 0,
  current: null,
  rows: freshRows(),
  failure: null,
})

let active = false
let generation = 0

function patchRow(key: StepKey, patch: Partial<StepRow>) {
  runStore.set((s) => ({ rows: { ...s.rows, [key]: { ...s.rows[key], ...patch } } }))
}

function complete(key: StepKey | null) {
  if (!key) return
  const row = runStore.get().rows[key]
  if (row.state !== 'running') return
  patchRow(key, { state: 'done', ms: performance.now() - row.t0 })
  if (runStore.get().current === key) runStore.set({ current: null })
}

export const steps = {
  begin(label: string) {
    this.reset()
    active = true
    generation += 1
    runStore.set({ visible: true, leaving: false, label, t0: performance.now(), current: null, rows: freshRows(), failure: null })
    // The sheet takes the workspace's main column for the run's duration; the
    // previous run's review steps aside. The review tab is where the sheet
    // lives, so a run always brings it into view.
    reviewStore.set({ current: null, view: 'hidden', selected: null, diff: null })
    showTab('review')
  },

  // mark the running step done, start `key`, optionally with a detail line
  start(key: StepKey, text?: string) {
    if (!active) return
    const { current } = runStore.get()
    if (current !== key) {
      complete(current)
      patchRow(key, { state: 'running', t0: performance.now(), ms: null })
      runStore.set({ current: key })
    }
    if (text !== undefined) this.detail(key, text)
  },

  // live text under the running step; ignored for every other step
  detail(key: StepKey, text: string) {
    if (!active || runStore.get().current !== key) return
    patchRow(key, { sub: text })
  },

  // sub-text without the running-step gate: a final summary that is recorded
  // once a later step has taken over (the compile PDF line) would otherwise
  // be dropped by detail()
  note(key: StepKey, text: string) {
    if (!active) return
    patchRow(key, { sub: text })
  },

  // A worker announced one TeX Live file download (the kpathsea hook logs
  // "Start downloading texlive file …" per request). Tally them on the
  // running step; the hook fetches synchronously one at a time, so the count
  // is exactly the files asked for so far.
  texDownload() {
    const { current, rows } = runStore.get()
    if (!active || !current) return
    patchRow(current, { downloads: rows[current].downloads + 1 })
  },

  complete(key: StepKey) {
    complete(key)
  },

  done(key: StepKey, text?: string) {
    if (!active) return
    if (runStore.get().current !== key) this.start(key)
    if (text !== undefined) this.detail(key, text)
    complete(key)
  },

  skip(key: StepKey) {
    if (!active) return
    if (runStore.get().current === key) complete(key)
    patchRow(key, { state: 'skipped', ms: null })
  },

  // a finished run: everything resolved, then the sheet gives way to the
  // success screen — first the leave-fade, then the unmount, so the success
  // screen never animates in underneath the dying progress view
  finish() {
    if (!active) return
    active = false
    complete(runStore.get().current)
    runStore.set((s) => ({
      rows: Object.fromEntries(
        Object.entries(s.rows).map(([key, row]) => [key, row.state === 'pending' ? { ...row, state: 'done' } : row]),
      ) as Record<StepKey, StepRow>,
    }))
    const mine = generation
    setTimeout(() => {
      if (mine !== generation) return  // a re-run grabbed the sheet
      runStore.set({ leaving: true })
      setTimeout(() => {
        if (mine !== generation || active) return
        runStore.set({ visible: false, leaving: false })
        const review = reviewStore.get()
        if (review.current && review.view !== 'diff') {
          reviewStore.set({ view: 'done' })
          showTab('review')
        }
      }, 500)
    }, 450)
  },

  // a failed run: the sheet stays as a post-mortem of where it stopped
  fail(text: string) {
    if (!active) return
    active = false
    const { current, rows } = runStore.get()
    if (current) patchRow(current, { state: 'failed', ms: performance.now() - rows[current].t0 })
    runStore.set({ current: null, failure: text })
  },

  reset() {
    active = false
    generation += 1
    runStore.set({ visible: false, leaving: false, current: null, rows: freshRows(), failure: null })
  },
}
