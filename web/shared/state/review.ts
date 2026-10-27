import { createStore } from '../lib/store'
import type { DiffRow } from '../lib/diff'

export type ChangeStatus = 'M' | 'D' | 'A' | 'R' | 'U'

export interface Dimensions {
  w: number
  h: number
}

// One file of the comparison between the original folder and /cleaned.
// origText/newText are filled lazily (and cached) when its diff is opened.
export interface Change {
  rel: string
  origFile: File | null
  outPath: string | null
  origSize: number | null
  outSize: number | null
  status: ChangeStatus
  origText: string | null
  newText: string | null
  renamedFrom?: string
  resized?: { before: Dimensions; after: Dimensions }
}

export interface Review {
  stats: Record<ChangeStatus, number>
  changes: Change[]
  unchanged: Change[]
  archive: { path: string; size: number } | null
}

// The outcome of the pixel compare, as the success screen states it.
export type CompareOutcome =
  | { state: 'ok'; text: string; pages: number; ms: number }
  | { state: 'differs'; text: string; differing: number; pages: number; ms: number }
  | { state: 'failed'; text: string }

export interface PipelineConfig {
  input: string
  target: string
  keep_bib: boolean
  clean_classes: boolean
  no_zzrm: boolean
  skip_watermark: boolean
  resize_images: boolean
  im_size: number
  tar: string | null
  force: boolean
  log_level: string
  user_provided_main_files?: string[]
}

export interface CompileRun {
  main: string
  mains: string[]
  compileMs: string
  log: string
  inputs: Set<string>
  outputs: Set<string>
  pdf: Uint8Array
}

export interface CurrentReview {
  review: Review
  cfg: PipelineConfig
  totalMs: number
  run: CompileRun
  compare?: CompareOutcome
}

export type DiffMode = 'sbs' | 'uni'

export type DiffView =
  | { state: 'loading'; entry: Change }
  | { state: 'error'; entry: Change; message: string }
  | { state: 'binary'; entry: Change }
  | {
      state: 'ready'
      entry: Change
      rows: DiffRow[]
      aLines: string[]
      bLines: string[]
      counts: { del: number; ins: number; oldN: number; newN: number }
      expanded: ReadonlySet<string>
      allExpanded: boolean
    }

// What the review tab's main column shows once no run is in progress: the
// empty note, the run's success screen, or the diff of the picked file.
export type ReviewView = 'empty' | 'done' | 'diff' | 'hidden'

export interface ReviewState {
  current: CurrentReview | null
  view: ReviewView
  selected: string | null
  diff: DiffView | null
  diffMode: DiffMode | null
}

export const reviewStore = createStore<ReviewState>({
  current: null,
  view: 'empty',
  selected: null,
  diff: null,
  diffMode: null,
})
