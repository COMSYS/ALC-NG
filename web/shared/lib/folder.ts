import { logErr, logInfo, logv } from '../state/log'
import { IDLE_STATUS, setFolderStatus, setSubmissionOpen, uiStore } from '../state/ui'
import { fmtKB, plural } from './format'
import { baseOf, dirOf, encPath, stripTexComments } from './paths'
import { resetReview } from './review'
import { SAMPLES } from './samples'

// The submission currently loaded: relative path → File.
export interface Folder {
  files: Map<string, File>
  label: string
}

let folder: Folder | null = null

export const currentFolder = () => folder

// A File that remembers where it sat in the picked folder. Synthesized files
// (samples, drag and drop) have no webkitRelativePath of their own.
function withRelativePath(file: File, path: string): File {
  Object.defineProperty(file, 'webkitRelativePath', { value: path })
  return file
}

// Builds the folder file map from a folder selection (any list of File
// objects whose webkitRelativePath starts with the folder's own name) and
// works out what can be run on it.
export async function setFolder(fileList: Iterable<File>, label: string) {
  const files = new Map<string, File>()
  for (const file of fileList) {
    const parts = (file.webkitRelativePath || file.name).split('/')
    parts.shift()  // strip the selected folder's own name
    const rel = parts.join('/')
    if (rel) files.set(rel, file)
  }
  folder = { files, label }
  const sampleIndex = SAMPLES.findIndex((s) => s.label === label)
  uiStore.set({ folder: { label, count: files.size }, sample: sampleIndex < 0 ? null : sampleIndex })

  setSubmissionOpen(true)  // a fresh selection is handled with the controls open
  resetReview()
  let total = 0
  for (const f of files.values()) total += f.size
  logInfo(`folder "${label}": ${files.size} file(s), ${fmtKB(total)} total`)
  for (const [rel, file] of files) logv(`  ${rel} (${fmtKB(file.size)})`)

  if (files.size === 0) {
    setFolderStatus('The selected folder is empty.', 'err')
    uiStore.set({ mainFiles: [], mainSel: [] })
    return
  }
  if ((await refreshMainFiles()) === 0) return  // the reason is already on the status line
  // A run always compiles first and derives the recorder from the engine's
  // access log; a submitted .fls belongs to an older build and is not staged.
  setFolderStatus(`${files.size} ${plural(files.size, 'file')} loaded.`, 'ok')
}

// The files of an <input webkitdirectory> selection.
export function setFolderFromInput(list: FileList) {
  const files = [...list]
  if (files.length === 0) return
  void setFolder(files, files[0].webkitRelativePath.split('/')[0] || 'folder')
}

// The files of a drop: a single folder is the submission itself; anything
// else (several folders, loose files) becomes one submission named
// "dropped files".
export async function setFolderFromDrop(dt: DataTransfer) {
  const entries = [...dt.items]
    .map((item) => item.webkitGetAsEntry?.())
    .filter((entry): entry is FileSystemEntry => !!entry)
  if (entries.length === 0) return
  const single = entries.length === 1 && entries[0].isDirectory
  const label = single ? entries[0].name : 'dropped files'
  const files: File[] = []
  for (const entry of entries) {
    await collectEntry(entry, single ? '' : `${entry.name}`, files, label)
  }
  await setFolder(files, label)
}

async function collectEntry(entry: FileSystemEntry, rel: string, out: File[], root: string) {
  if (entry.isFile) {
    const file = await new Promise<File>((resolve, reject) => (entry as FileSystemFileEntry).file(resolve, reject))
    out.push(withRelativePath(file, `${root}/${rel}`))
    return
  }
  const reader = (entry as FileSystemDirectoryEntry).createReader()
  // readEntries hands out a directory in batches until it returns none
  for (;;) {
    const batch = await new Promise<FileSystemEntry[]>((resolve, reject) => reader.readEntries(resolve, reject))
    if (batch.length === 0) break
    for (const child of batch) await collectEntry(child, rel ? `${rel}/${child.name}` : child.name, out, root)
  }
}

// Drops the current selection, leaving the flags as they are.
export function clearFolder() {
  folder = null
  uiStore.set({ folder: null, mainFiles: [], mainSel: [], status: IDLE_STATUS, sample: null })
  setSubmissionOpen(true)
  resetReview()
  logInfo('folder selection cleared')
}

// Folders are listed by the server (its /tree/ endpoint), so a fixture needs
// one line in SAMPLES rather than a transcription of its whole file tree.
async function sampleListing(dir: string): Promise<string[]> {
  const response = await fetch(`/tree${encPath(dir)}`)
  if (!response.ok) throw new Error(`list ${dir}: ${response.status}`)
  const { files } = (await response.json()) as { files: string[] }
  logv(`/tree: ${files.length} file(s) under ${dir}`)
  return files
}

// Fetches a bundled sample folder so the page can be tried without a folder.
export async function loadSampleFolder(index: number) {
  const sample = SAMPLES[index]
  if (!sample) return
  uiStore.set({ sample: index, sampleLoading: true })
  setFolderStatus(`Loading the ${sample.pick} sample…`)
  try {
    const files: File[] = []
    for (const rel of sample.files ?? (await sampleListing(sample.dir))) {
      const response = await fetch(`${sample.dir}/${encPath(rel)}`)
      if (!response.ok) {
        logErr(`sample fetch failed: ${rel} (${response.status})`)
        throw new Error(`fetch ${rel}: ${response.status}`)
      }
      // the synthetic folder name is stripped again by setFolder()
      files.push(withRelativePath(new File([await response.arrayBuffer()], baseOf(rel)), `sample/${rel}`))
    }
    logv(`fetched ${files.length} sample file(s) from ${sample.dir}`)
    await setFolder(files, sample.label)
  } catch (err) {
    logErr(`sample load failed: ${(err as Error).message}`)
    uiStore.set({ sample: null })
    setFolderStatus(`The sample could not be loaded: ${(err as Error).message}`, 'err')
  }
  uiStore.set({ sampleLoading: false })
}

export interface MainCandidate {
  rel: string
  cls: string
}

// Every .tex file with a \documentclass, best main file first. Native alc-ng
// only considers top-level files (helper::find_mains); a nested one is a
// choice the user makes here, and `standalone` documents are usually figures
// that the real document \includes.
export function rankMainCandidates(found: MainCandidate[]): MainCandidate[] {
  const rank = ({ rel, cls }: MainCandidate) =>
    (rel.includes('/') ? 100 : 0) +
    (cls === 'standalone' ? 50 : 0) +
    (/^(main|ms|paper|article|report|thesis)\.tex$/i.test(baseOf(rel)) ? 0 : 10) +
    rel.split('/').length
  return [...found].sort((a, b) => rank(a) - rank(b))
}

export const documentClassOf = (text: string) =>
  /\\documentclass\s*(?:\[[^\]]*\])?\s*\{([^}]*)\}/.exec(stripTexComments(text))?.[1].trim() ?? null

async function mainFileCandidates(): Promise<MainCandidate[]> {
  if (!folder) return []
  const found: MainCandidate[] = []
  for (const rel of [...folder.files.keys()].sort()) {
    if (!/\.tex$/i.test(rel)) continue
    const cls = documentClassOf(await folder.files.get(rel)!.text())
    if (cls !== null) found.push({ rel, cls })
  }
  return rankMainCandidates(found)
}

// Fills the main-file checklist with the candidates of the current folder and
// preselects the best one. Returns how many there were.
async function refreshMainFiles(): Promise<number> {
  const candidates = await mainFileCandidates()
  uiStore.set({
    mainFiles: candidates.map(({ rel, cls }) => ({ value: rel, label: `${rel} (${cls})` })),
    mainSel: candidates[0] ? [candidates[0].rel] : [],
  })
  if (candidates.length === 0) {
    setFolderStatus('No .tex file in this folder has a \\documentclass, so there is nothing to compile.', 'err')
    return 0
  }
  logInfo(`main file candidates: ${candidates.map((c) => c.rel).join(', ')} — using ${candidates[0].rel}`)
  if (candidates[0].rel.includes('/')) {
    logInfo(
      `no top-level main file (native alc-ng would need --main-files ${candidates[0].rel}); it compiles in ${dirOf(candidates[0].rel)}/`,
    )
  }
  return candidates.length
}

// The mains the user checked — a single candidate runs unlisted, several are
// checked off and all are cleaned around (native alc-ng's --main-files list).
export function findMainFiles(): string[] {
  const sel = uiStore.get().mainSel.filter((rel) => folder?.files.has(rel))
  if (sel.length === 0) throw new Error('no main file selected')
  return sel
}
