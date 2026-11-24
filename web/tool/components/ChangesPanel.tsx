import { fmtSize } from '../../shared/lib/format'
import { baseOf, dirOf } from '../../shared/lib/paths'
import { dimsLabel, downloadArchive, openDiff } from '../../shared/lib/review'
import { useStore } from '../../shared/lib/store'
import { reviewStore, type Change, type Review } from '../../shared/state/review'
import { ChevronIcon } from './icons'

// The changed files of the last run, grouped by folder like a git status;
// a click opens the file's diff in the workspace.
export function ChangesPanel() {
  const current = useStore(reviewStore, (s) => s.current)
  const selected = useStore(reviewStore, (s) => s.selected)
  if (!current) return null
  const { review } = current
  const s = review.stats
  const parts: [number, string][] = [
    [s.M, 'modified'],
    [s.D, 'removed'],
    [s.A, 'added'],
    [s.R, 'renamed'],
  ]

  return (
    <section className="sideSection">
      <div className="sectionHead">
        <h2>Changes</h2>
        <span className="count">{review.changes.length} files</span>
      </div>
      <p className="tally">
        {parts
          .filter(([n]) => n > 0)
          .map(([n, label], i) => (
            <span key={label}>
              {i > 0 && ', '}
              <b>{n}</b> {label}
            </span>
          ))}
        {s.U > 0 && (
          <span>
            {parts.some(([n]) => n > 0) && ', '}
            <b>{s.U}</b> unchanged
          </span>
        )}
      </p>
      <Tree review={review} selected={selected} />
      {review.archive && (
        <div className="archive">
          <span className="name">{baseOf(review.archive.path)}</span>
          <span className="size">{fmtSize(review.archive.size)}</span>
          <button type="button" className="linkButton" onClick={downloadArchive}>
            Download
          </button>
        </div>
      )}
    </section>
  )
}

function Tree({ review, selected }: { review: Review; selected: string | null }) {
  const byDir = new Map<string, Change[]>()
  for (const c of review.changes) {
    const dir = dirOf(c.rel)
    byDir.set(dir, [...(byDir.get(dir) ?? []), c])
  }
  const dirs = [...byDir.keys()].sort((a, b) => (a === '' ? -1 : b === '' ? 1 : a < b ? -1 : 1))

  return (
    <div className="tree">
      {dirs.map((dir) =>
        dir === '' ? (
          byDir.get('')!.map((c) => <FileRow key={c.rel} change={c} selected={selected === c.rel} />)
        ) : (
          <details className="treeDir" open key={dir}>
            <summary>
              <ChevronIcon />
              {dir}/
            </summary>
            {byDir.get(dir)!.map((c) => (
              <FileRow key={c.rel} change={c} selected={selected === c.rel} />
            ))}
          </details>
        ),
      )}
      {review.unchanged.length > 0 && (
        <details className="treeDir treeUnchanged">
          <summary>
            <ChevronIcon />
            {review.unchanged.length} unchanged
          </summary>
          {review.unchanged.map((c) => (
            <FileRow key={c.rel} change={c} selected={selected === c.rel} />
          ))}
        </details>
      )}
    </div>
  )
}

const STATUS_NAMES: Record<Change['status'], string> = {
  M: 'modified',
  D: 'removed',
  A: 'added',
  R: 'renamed',
  U: 'unchanged',
}

function FileRow({ change: c, selected }: { change: Change; selected: boolean }) {
  const title = [`${c.rel} (${STATUS_NAMES[c.status]})`]
  if (c.renamedFrom) title.push(`renamed from ${c.renamedFrom}`)
  if (c.resized) title.push(`resized from ${dimsLabel(c.resized.before)} to ${dimsLabel(c.resized.after)} px`)
  const note =
    c.status === 'D'
      ? fmtSize(c.origSize ?? 0)
      : c.status === 'A'
        ? fmtSize(c.outSize ?? 0)
        : c.status === 'R' && c.renamedFrom
          ? `was ${dirOf(c.renamedFrom) === dirOf(c.rel) ? baseOf(c.renamedFrom) : `${dirOf(c.renamedFrom)}/`}`
          : `${fmtSize(c.origSize ?? 0)} → ${fmtSize(c.outSize ?? 0)}`
  return (
    <button type="button" className={`treeFile${selected ? ' sel' : ''}`} title={title.join('\n')} onClick={() => void openDiff(c)}>
      <span className={`st st-${c.status}`}>{c.status}</span>
      <span className="name">{baseOf(c.rel)}</span>
      {c.resized && <span className="tag">resized</span>}
      <span className="note">{note}</span>
    </button>
  )
}
