import { logErr, logInfo, logOk, logv } from '../state/log'
import {
  reviewStore,
  type Change,
  type ChangeStatus,
  type CompileRun,
  type Dimensions,
  type DiffMode,
  type PipelineConfig,
  type Review,
} from '../state/review'
import { steps } from '../state/steps'
import { setFolderStatus, showTab, uiStore } from '../state/ui'
import { cleanedBytes, listEntries, texCall } from './engine'
import type { Folder } from './folder'
import { fmtSize } from './format'
import { lineOps, pairRows, splitLines } from './diff'
import { saveBlob } from './net'
import { baseOf } from './paths'

// --- review: file-tree diff between the original folder and /cleaned ---------
//
// The used-file decision belongs to the pipeline; here we only compare the two
// trees: what survived (modified/unchanged), what the cleaner dropped ("D" —
// in the original, absent from the cleaned tree) and what appeared.

const TEXT_OUTPUT_EXT = /\.(tex|sty|cls|bbl|bib|txt|fls|aux|log|md|json|ini)$/i
const DIFF_MAX_BYTES = 400000  // above this, treat as opaque
const RASTER_IMAGE_RE = /\.(png|jpe?g|gif|tiff?|bmp|webp|avif)$/i

export function resetReview() {
  steps.reset()
  reviewStore.set({ current: null, view: 'empty', selected: null, diff: null })
  showTab('review')
}

export function isTextLike(rel: string, bytes: Uint8Array): boolean {
  if (!TEXT_OUTPUT_EXT.test(rel) || bytes.length > DIFF_MAX_BYTES) return false
  // A quick binary sniff: NUL bytes in the first 2 KiB.
  const probe = bytes.subarray(0, 2048)
  for (let i = 0; i < probe.length; i++) if (probe[i] === 0) return false
  return true
}

function bytesEqual(a: Uint8Array, b: Uint8Array): boolean {
  if (a.length !== b.length) return false
  for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return false
  return true
}

const decode = (bytes: Uint8Array) => new TextDecoder().decode(bytes)

// --- resized images ----------------------------------------------------------
//
// resize_images rewrites a raster image in place (same name, new pixel size),
// so a size delta alone cannot tell a downscale from a re-encode. Both sides
// are decoded in the page and a file whose dimensions changed is marked, so
// the changes tree can say "resized" instead of leaving it as another "M".

async function imageDimensions(blob: Blob): Promise<Dimensions | null> {
  try {
    const bitmap = await createImageBitmap(blob)
    const dims = { w: bitmap.width, h: bitmap.height }
    bitmap.close?.()
    return dims
  } catch {
    return null  // not a format this browser decodes — no claim either way
  }
}

async function markResizedImages(changes: Change[]) {
  const candidates = changes.filter((c) => c.status === 'M' && c.origFile && c.outPath && RASTER_IMAGE_RE.test(c.rel))
  let resized = 0
  for (const c of candidates) {
    const before = await imageDimensions(c.origFile!)
    if (!before) continue
    const after = await imageDimensions(new Blob([(await cleanedBytes(c.outPath!)) as BlobPart]))
    if (!after) continue
    if (after.w !== before.w || after.h !== before.h) {
      c.resized = { before, after }
      resized += 1
    }
  }
  if (resized > 0) {
    logInfo(`resize_images: ${resized} of ${candidates.length} changed image(s) downscaled (${uiStore.get().flags.imSize} px target)`)
  } else if (candidates.length > 0) {
    logv(`resize_images: none of the ${candidates.length} changed image(s) changed dimensions`)
  }
}

export const dimsLabel = (d: Dimensions) => `${d.w}×${d.h}`

export async function buildReview(folder: Folder, cfg: PipelineConfig, totalMs: number, run: CompileRun) {
  steps.detail('review', 'Reading the cleaned files')
  const entries = listEntries(await texCall({ cmd: 'alcng_list', path: cfg.target }, ['alcng_list']))

  // Cleaned tree, keyed by submission-relative path.
  const cleaned = new Map(
    entries.map((e) => [e.path.startsWith(`${cfg.target}/`) ? e.path.slice(cfg.target.length + 1) : e.path, e]),
  )

  const rels = [...new Set([...folder.files.keys(), ...cleaned.keys()])].sort()
  const review: Review = { stats: { M: 0, D: 0, A: 0, R: 0, U: 0 }, changes: [], unchanged: [], archive: null }

  for (const [i, rel] of rels.entries()) {
    if (i % 16 === 0) steps.detail('review', `File ${i} of ${rels.length}`)
    const orig = folder.files.get(rel) ?? null
    const out = cleaned.get(rel) ?? null
    let status: ChangeStatus
    let origText: string | null = null
    let newText: string | null = null
    if (orig && out) {
      if (orig.size !== out.size) {
        status = 'M'
      } else {
        const ob = new Uint8Array(await orig.arrayBuffer())
        const nb = await cleanedBytes(out.path)
        if (bytesEqual(ob, nb)) status = 'U'
        else {
          status = 'M'
          origText = isTextLike(rel, ob) ? decode(ob) : null
          newText = isTextLike(rel, nb) ? decode(nb) : null
        }
      }
    } else {
      status = out ? 'A' : 'D'
    }
    const change: Change = {
      rel,
      origFile: orig,
      outPath: out?.path ?? null,
      origSize: orig?.size ?? null,
      outSize: out?.size ?? null,
      status,
      origText,
      newText,
    }
    review.stats[status] += 1
    ;(status === 'U' ? review.unchanged : review.changes).push(change)
  }

  // arXiv upload rename (native file_renames.rs): with tar configured the
  // cleaner writes "Section 2/x.png" to "Section_2/x.png" and rewrites the
  // references to match — unless the underscore path is already taken, which
  // is why such an added entry can only ever be the cleaner renaming its
  // spaced twin. Fold each dropped+appeared pair into a single "R" row.
  const addedByRel = new Map(review.changes.filter((c) => c.status === 'A').map((c) => [c.rel, c]))
  const twins = new Set<Change>()
  for (const c of review.changes) {
    if (c.status !== 'D' || !c.rel.includes(' ')) continue
    const twin = addedByRel.get(c.rel.replace(/ /g, '_'))
    if (!twin || twins.has(twin)) continue
    twins.add(twin)
    c.status = 'R'
    c.renamedFrom = c.rel
    c.rel = twin.rel
    c.outPath = twin.outPath
    c.outSize = twin.outSize
    review.stats.D -= 1
    review.stats.A -= 1
    review.stats.R += 1
  }
  if (twins.size) review.changes = review.changes.filter((c) => !twins.has(c))

  // With resize_images on, a changed image may have been downscaled rather
  // than merely re-encoded; the tree says so, which needs both dimensions.
  if (cfg.resize_images) {
    steps.detail('review', 'Measuring resized images')
    await markResizedImages(review.changes)
  }

  if (typeof cfg.tar === 'string' && cfg.tar !== '-') {
    try {
      const archive = listEntries(await texCall({ cmd: 'alcng_list', path: cfg.tar, quiet: true }, ['alcng_list']))[0]
      if (archive) review.archive = { path: archive.path, size: archive.size }
    } catch {
      logv(`no tar archive found at ${cfg.tar}`)
    }
  }

  // The success screen is built now but only revealed once the progress
  // sheet has faded out (steps.finish).
  reviewStore.set({ current: { review, cfg, totalMs, run } })
  logOk(`alc-ng pipeline done in ${(totalMs / 1000).toFixed(1)} s: ${entries.length} output file(s)`)
  const s = review.stats
  steps.detail('review', `${s.M} modified, ${s.D} removed, ${s.A} added, ${s.U} unchanged`)
  setFolderStatus(
    `Cleaned: ${entries.length} ${entries.length === 1 ? 'file' : 'files'}, ${fmtSize(entries.reduce((n, e) => n + e.size, 0))}.`,
    'ok',
  )
}

// --- statistics: what the cleaner did, as figures -------------------------
//
// The review answers "which files changed"; this answers "how much". Every
// number derives from the same file lists the review uses (sizes only), so
// the success screen and the statistics tab never disagree.

const KIND_BY_EXT: Record<string, string> = {
  png: 'figures', jpg: 'figures', jpeg: 'figures', gif: 'figures', tif: 'figures', tiff: 'figures',
  pdf: 'figures', eps: 'figures', ps: 'figures', svg: 'figures',
  tex: 'tex sources',
  bib: 'bibliography', bbl: 'bibliography',
  sty: 'styles & classes', cls: 'styles & classes', def: 'styles & classes', fd: 'styles & classes',
  pygtex: 'minted cache', pyg: 'minted cache', pygstyle: 'minted cache',
}

export const KIND_TONE: Record<string, string> = {
  'tex sources': 'var(--color-text)',
  figures: 'var(--color-warn)',
  'minted cache': 'var(--color-accent)',
  bibliography: 'var(--color-success)',
  'styles & classes': 'var(--color-secondary)',
  other: 'var(--color-faint)',
}

export function kindOf(rel: string): string {
  const m = /\.([A-Za-z0-9]+)$/.exec(rel)
  return (m && KIND_BY_EXT[m[1].toLowerCase()]) || 'other'
}

export interface KindRow {
  kind: string
  origN: number
  origB: number
  outN: number
  outB: number
  removed: number
}

export interface Stats {
  origBytes: number
  cleanedBytes: number
  savedBytes: number
  droppedBytes: number
  trimmedBytes: number
  addedBytes: number
  kindRows: KindRow[]
  reductions: { rel: string; status: ChangeStatus; saved: number }[]
  stats: Review['stats']
  totalMs: number
  archive: Review['archive']
  origN: number
  outN: number
}

export function computeStats(review: Review, totalMs: number): Stats {
  const all = [...review.changes, ...review.unchanged]
  const origBytes = all.reduce((n, c) => n + (c.origSize ?? 0), 0)
  const cleanedTotal = all.reduce((n, c) => n + (c.outSize ?? 0), 0)
  // The composition split: whole files dropped, bytes trimmed out of kept
  // files, and bytes the cleaner added (new files, a kept file that grew).
  let droppedBytes = 0
  let trimmedBytes = 0
  let addedBytes = 0
  for (const c of all) {
    const before = c.origSize ?? 0
    const after = c.outSize ?? 0
    if (c.outSize === null) droppedBytes += before
    else if (after < before) trimmedBytes += before - after
    else addedBytes += after - before
  }
  const kinds = new Map<string, Omit<KindRow, 'kind' | 'removed'>>()
  for (const c of all) {
    const k = kindOf(c.rel)
    let e = kinds.get(k)
    if (!e) kinds.set(k, (e = { origN: 0, origB: 0, outN: 0, outB: 0 }))
    if (c.origSize !== null) {
      e.origN++
      e.origB += c.origSize
    }
    if (c.outSize !== null) {
      e.outN++
      e.outB += c.outSize
    }
  }
  const kindRows = [...kinds.entries()]
    .map(([kind, e]) => ({ kind, ...e, removed: e.origB - e.outB }))
    .filter((e) => e.origB > 0)
    .sort((a, b) => b.origB - a.origB)
  const reductions = all
    .map((c) => ({ rel: c.rel, status: c.status, saved: (c.origSize ?? 0) - (c.outSize ?? 0) }))
    .filter((r) => r.saved > 0)
    .sort((a, b) => b.saved - a.saved)
  return {
    origBytes,
    cleanedBytes: cleanedTotal,
    savedBytes: origBytes - cleanedTotal,
    droppedBytes,
    trimmedBytes,
    addedBytes,
    kindRows,
    reductions,
    stats: review.stats,
    totalMs,
    archive: review.archive,
    origN: all.filter((c) => c.origSize !== null).length,
    outN: all.filter((c) => c.outSize !== null).length,
  }
}

// One-line summaries of a run, shared by the result and the statistics.
export const sizeLine = (stats: Stats) => {
  const pct = stats.origBytes > 0 ? (stats.savedBytes / stats.origBytes) * 100 : 0
  return `${fmtSize(stats.origBytes)} → ${fmtSize(stats.cleanedBytes)} (${pct.toFixed(pct < 10 ? 1 : 0)}% smaller)`
}

export const filesLine = (stats: Stats) => {
  const s = stats.stats
  const parts = [`${s.M} modified`, `${s.D} removed`, `${s.A} added`]
  if (s.R) parts.push(`${s.R} renamed`)
  parts.push(`${s.U} unchanged`)
  return parts.join(', ')
}

// --- the diff of one file ------------------------------------------------------

// Default pane mode at the moment a diff is first opened (user toggles win).
const defaultMode = (): DiffMode => (window.matchMedia('(max-width: 62rem)').matches ? 'uni' : 'sbs')

// Opens the diff of `change` in the review tab. A newer pick supersedes a
// slower load still in flight.
export async function openDiff(change: Change) {
  showTab('review')
  reviewStore.set((s) => ({
    view: 'diff',
    selected: change.rel,
    diff: { state: 'loading', entry: change },
    diffMode: s.diffMode ?? defaultMode(),
  }))
  const stillCurrent = () => reviewStore.get().diff?.entry === change
  try {
    const texts = await loadTexts(change)
    if (!stillCurrent()) return
    if (texts === null) {
      reviewStore.set({ diff: { state: 'binary', entry: change } })
      return
    }
    const aLines = splitLines(texts.oldText)
    const bLines = splitLines(texts.newText)
    const rows = pairRows(lineOps(aLines, bLines))
    const counts = {
      del: rows.filter((r) => r.kind === 'pair' && r.a !== null).length,
      ins: rows.filter((r) => r.kind === 'pair' && r.b !== null).length,
      oldN: aLines.length,
      newN: bLines.length,
    }
    reviewStore.set({
      diff: { state: 'ready', entry: change, rows, aLines, bLines, counts, expanded: new Set(), allExpanded: false },
    })
    logv(`diff opened: ${change.rel} (${change.status}) +${counts.ins} −${counts.del}`)
  } catch (err) {
    const message = String((err as Error)?.message ?? err)
    logErr(`diff failed for ${change.rel}: ${message}`)
    if (stillCurrent()) reviewStore.set({ diff: { state: 'error', entry: change, message } })
  }
}

// Both sides as text, or null when either side is binary.
async function loadTexts(change: Change): Promise<{ oldText: string; newText: string } | null> {
  let oldText = ''
  let newText = ''
  if (change.status !== 'A' && change.origFile) {
    if (change.origText === null) {
      const ob = new Uint8Array(await change.origFile.arrayBuffer())
      if (!isTextLike(change.rel, ob)) return null
      change.origText = decode(ob)
    }
    oldText = change.origText
  }
  if (change.status !== 'D' && change.outPath) {
    if (change.newText === null) {
      const nb = await cleanedBytes(change.outPath)
      if (!isTextLike(change.rel, nb)) return null
      change.newText = decode(nb)
    }
    newText = change.newText
  }
  return { oldText, newText }
}

// Back from the diff to the success screen (the diff header's ← summary).
export function showDone() {
  if (reviewStore.get().current) reviewStore.set({ view: 'done' })
}

export function setDiffMode(mode: DiffMode) {
  reviewStore.set({ diffMode: mode })
}

export function expandDiffRange(key: string) {
  const diff = reviewStore.get().diff
  if (diff?.state !== 'ready') return
  reviewStore.set({ diff: { ...diff, expanded: new Set([...diff.expanded, key]) } })
}

export function toggleExpandAll() {
  const diff = reviewStore.get().diff
  if (diff?.state !== 'ready') return
  reviewStore.set({ diff: { ...diff, allExpanded: !diff.allExpanded, expanded: diff.allExpanded ? new Set() : diff.expanded } })
}

// The most interesting file to open first: modified, then dropped, added,
// renamed.
export const firstChange = (review: Review) =>
  review.changes.find((c) => c.status === 'M') ??
  review.changes.find((c) => c.status === 'D') ??
  review.changes.find((c) => c.status === 'A') ??
  review.changes.find((c) => c.status === 'R')

// Hand a cleaned file (or the tarball) to the browser: pulled out of the
// engine's memfs on demand.
export async function downloadCleaned(path: string, name = baseOf(path)) {
  try {
    saveBlob((await cleanedBytes(path)) as BlobPart, name)
  } catch (err) {
    logErr(`download of ${path} failed: ${(err as Error).message}`)
  }
}

export function downloadArchive() {
  const archive = reviewStore.get().current?.review.archive
  if (archive) void downloadCleaned(archive.path)
}
