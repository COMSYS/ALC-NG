import { createStore } from '../lib/store'

export type Tone = 'info' | 'success' | 'error'
export type Tab = 'review' | 'pdf' | 'compare' | 'stats'
export type EngineChoice = 'auto' | 'pdftex' | 'xetex'
export type BackendChoice = 'swiftlatex' | 'texlyre'
export type PipelineLogLevel = 'off' | 'info' | 'trace'

export interface Status {
  text: string
  tone: Tone
}

export interface Flags {
  keepBib: boolean
  cleanClasses: boolean
  noZzrm: boolean
  skipWatermark: boolean
  resizeImages: boolean
  tar: boolean
  force: boolean
  compare: boolean
  imSize: number
  diffColor: string
  logLevel: PipelineLogLevel
  engine: EngineChoice
  backend: BackendChoice
}

export interface MainFileOption {
  value: string
  label: string
}

export interface UiState {
  folder: { label: string; count: number } | null
  status: Status | null
  running: boolean
  sample: number | null
  sampleLoading: boolean
  // what the arXiv field holds, and whether its fetch is in flight
  arxiv: string
  arxivLoading: boolean
  // an archive (.zip, .tar.gz) is being read into a submission
  unpacking: boolean
  mainFiles: MainFileOption[]
  mainSel: string[]
  flags: Flags
  tab: Tab
  submissionOpen: boolean
  // the compiled PDF of the last run, shown on the pdf tab; `url` is for
  // opening and saving it
  pdf: { bytes: Uint8Array; url: string } | null
}

export const IDLE_STATUS: Status = {
  text: 'Choose a folder or an archive, fetch a paper from arXiv, or load a sample.',
  tone: 'info',
}

// One submission at a time: while any source is being read — a sample, an
// archive, an arXiv download — or a run is going, the controls are off.
export const uiBusy = (s: UiState) => s.running || s.sampleLoading || s.arxivLoading || s.unpacking

export const uiStore = createStore<UiState>({
  folder: null,
  status: IDLE_STATUS,
  running: false,
  sample: null,
  sampleLoading: false,
  arxiv: '',
  arxivLoading: false,
  unpacking: false,
  mainFiles: [],
  mainSel: [],
  flags: {
    keepBib: false,
    cleanClasses: false,
    noZzrm: false,
    skipWatermark: false,
    resizeImages: false,
    tar: true,
    force: false,
    compare: true,
    imSize: 512,
    diffColor: '#ff0000',
    logLevel: 'info',
    engine: 'auto',
    backend: 'swiftlatex',
  },
  tab: 'review',
  submissionOpen: true,
  pdf: null,
})

export function setFlag<K extends keyof Flags>(key: K, value: Flags[K]) {
  uiStore.set((s) => ({ flags: { ...s.flags, [key]: value } }))
}

// The panel's status line — an alert whose tone follows the message. Every
// pipeline step reports through here.
export function setFolderStatus(msg: string | null, cls?: 'ok' | 'err') {
  const tone: Tone = cls === 'ok' ? 'success' : cls === 'err' ? 'error' : 'info'
  uiStore.set({ status: msg ? { text: msg, tone } : null })
}

export const showTab = (tab: Tab) => uiStore.set({ tab })

export const setArxiv = (arxiv: string) => uiStore.set({ arxiv })

// A run needs the controls only before it starts: once compile and clean
// have succeeded, the panel folds away by itself and the rail shows the
// changes tree (a failed run folds it back open — the status alert is in
// there).
export const setSubmissionOpen = (open: boolean) => uiStore.set({ submissionOpen: open })

// Show the compiled PDF in the pdf tab (rendered with pdf.js, see
// components/PdfViewer.tsx).
export function setPdfView(bytes: Uint8Array) {
  clearPdfView()
  uiStore.set({ pdf: { bytes, url: URL.createObjectURL(new Blob([bytes as BlobPart], { type: 'application/pdf' })) } })
}

export function clearPdfView() {
  const pdf = uiStore.get().pdf
  if (pdf) URL.revokeObjectURL(pdf.url)
  uiStore.set((s) => ({ pdf: null, tab: s.tab === 'pdf' ? 'review' : s.tab }))
}
