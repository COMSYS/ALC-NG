import { Fragment, type ReactNode } from 'react'
import { hunkSegments, intraLine, texDecor, type DiffRow, type Span } from '../lib/diff'
import { fmtSize } from '../lib/format'
import { baseOf } from '../lib/paths'
import { dimsLabel, downloadCleaned, expandDiffRange, setDiffMode, showDone, toggleExpandAll } from '../lib/review'
import { useStore } from '../lib/store'
import { reviewStore, type Change, type DiffMode, type DiffView as Diff } from '../state/review'

// Path label in diff headers: a renamed entry names both sides of the move.
const pathLabel = (e: Change) => (e.renamedFrom ? `${e.renamedFrom} → ${e.rel}` : e.rel)

const Spans = ({ spans }: { spans: Span[] }) => (
  <>
    {spans.map((s, i) =>
      s.cls === 'wdel' || s.cls === 'wins' ? (
        <mark key={i} className={s.cls}>
          {s.text}
        </mark>
      ) : s.cls ? (
        <span key={i} className={s.cls}>
          {s.text}
        </span>
      ) : (
        <Fragment key={i}>{s.text}</Fragment>
      ),
    )}
  </>
)

export function DiffView() {
  const diff = useStore(reviewStore, (s) => s.diff)
  const mode = useStore(reviewStore, (s) => s.diffMode) ?? 'sbs'
  if (!diff) return null
  if (diff.state === 'loading') return <p className="diffNote">Loading {diff.entry.rel}…</p>
  if (diff.state === 'error') return <p className="diffNote">Could not open the diff: {diff.message}</p>
  return (
    <div className="diff" data-mode={mode}>
      <DiffHeader diff={diff} mode={mode} />
      {diff.state === 'binary' ? <p className="diffNote">Binary file, no line diff.</p> : <DiffBody diff={diff} mode={mode} />}
    </div>
  )
}

function DiffHeader({ diff, mode }: { diff: Diff; mode: DiffMode }) {
  const e = diff.entry
  let stat: ReactNode = null
  if (diff.state === 'ready') {
    const c = diff.counts
    stat = (
      <span className="counts">
        <span className="plus">+{c.ins}</span> <span className="minus">−{c.del}</span>, {c.oldN} → {c.newN} lines
      </span>
    )
  } else if (diff.state === 'binary') {
    const parts: string[] = []
    if (e.origSize !== null) parts.push(`original ${fmtSize(e.origSize)}`)
    if (e.outSize !== null) parts.push(`cleaned ${fmtSize(e.outSize)}`)
    if (e.resized) parts.push(`resized ${dimsLabel(e.resized.before)} → ${dimsLabel(e.resized.after)} px`)
    stat = <span className="counts">{parts.join(', ')}</span>
  }
  return (
    <div className="diffHead">
      <button type="button" className="button small" onClick={showDone}>
        Summary
      </button>
      <span className={`st st-${e.status}`}>{e.status}</span>
      <span className="path">{pathLabel(e)}</span>
      {stat}
      <span className="spacer" />
      {diff.state === 'ready' && (
        <>
          <div className="segmented" role="group" aria-label="Diff layout">
            {(['sbs', 'uni'] as const).map((m) => (
              <button key={m} type="button" className="button small" aria-pressed={mode === m} onClick={() => setDiffMode(m)}>
                {m === 'sbs' ? 'Split' : 'Unified'}
              </button>
            ))}
          </div>
          <button type="button" className="button small" onClick={toggleExpandAll}>
            {diff.allExpanded ? 'Collapse' : 'Expand all'}
          </button>
        </>
      )}
      {e.outPath && (
        <button type="button" className="linkButton" onClick={() => void downloadCleaned(e.outPath!, baseOf(e.rel))}>
          Download
        </button>
      )}
    </div>
  )
}

function DiffBody({ diff, mode }: { diff: Extract<Diff, { state: 'ready' }>; mode: DiffMode }) {
  const { rows } = diff
  const segs = diff.allExpanded ? (rows.length ? [{ t: 'rows' as const, from: 0, to: rows.length - 1 }] : []) : hunkSegments(rows)
  const renamed = diff.entry.status === 'R'
  if (!segs) return <p className="diffNote">{renamed ? 'Renamed only; the content is unchanged.' : 'No line changes.'}</p>
  if (segs.length === 0) return <p className="diffNote">{renamed ? 'Renamed only; the file is empty.' : 'The file is empty.'}</p>
  return (
    <div className="diffBody">
      {segs.map((seg) => {
        const key = `${seg.from}-${seg.to}`
        if (seg.t === 'rows' || diff.expanded.has(key)) {
          return <Rows key={key} diff={diff} mode={mode} from={seg.from} to={seg.to} />
        }
        const n = seg.to - seg.from + 1
        return (
          <button key={key} type="button" className="dbar" onClick={() => expandDiffRange(key)}>
            Show {n} unchanged line{n === 1 ? '' : 's'}
          </button>
        )
      })}
    </div>
  )
}

function Rows({ diff, mode, from, to }: { diff: Extract<Diff, { state: 'ready' }>; mode: DiffMode; from: number; to: number }) {
  const out: ReactNode[] = []
  for (let i = from; i <= to; i++) out.push(<Row key={i} row={diff.rows[i]} a={diff.aLines} b={diff.bLines} mode={mode} />)
  return <>{out}</>
}

const Gutter = ({ sign, n, className = 'ln' }: { sign: string; n: number | null; className?: string }) => (
  <span className={className}>
    {n !== null && <i className="gm">{sign}</i>}
    {n ?? ''}
  </span>
)

function Row({ row, a, b, mode }: { row: DiffRow; a: string[]; b: string[]; mode: DiffMode }) {
  if (row.kind === 'eq') {
    const left = texDecor(a[row.a])
    if (mode === 'sbs') {
      return (
        <div className="drow">
          <div className="dcell left">
            <Gutter sign="−" n={row.a + 1} />
            <code>
              <Spans spans={left} />
            </code>
          </div>
          <div className="dcell right">
            <Gutter sign="+" n={row.b + 1} />
            <code>
              <Spans spans={texDecor(b[row.b])} />
            </code>
          </div>
        </div>
      )
    }
    return (
      <div className="urow">
        <Gutter sign="−" n={row.a + 1} className="ln old" />
        <Gutter sign="+" n={row.b + 1} className="ln new" />
        <code>
          <Spans spans={left} />
        </code>
      </div>
    )
  }

  const oldLine = row.a !== null ? a[row.a] : null
  const newLine = row.b !== null ? b[row.b] : null
  if (mode === 'uni') {
    return (
      <>
        {row.a !== null && (
          <div className="urow has-del">
            <Gutter sign="−" n={row.a + 1} className="ln old" />
            <span className="ln new" />
            <code>{oldLine}</code>
          </div>
        )}
        {row.b !== null && (
          <div className="urow has-ins">
            <span className="ln old" />
            <Gutter sign="+" n={row.b + 1} className="ln new" />
            <code>{newLine}</code>
          </div>
        )}
      </>
    )
  }
  const words = intraLine(oldLine, newLine)
  return (
    <div className={`drow${row.a !== null ? ' has-del' : ''}${row.b !== null ? ' has-ins' : ''}`}>
      <div className={`dcell left${row.a === null ? ' filler' : ''}`}>
        <Gutter sign="−" n={row.a !== null ? row.a + 1 : null} />
        <code>{oldLine !== null && (words ? <Spans spans={words.a} /> : oldLine)}</code>
      </div>
      <div className={`dcell right${row.b === null ? ' filler' : ''}`}>
        <Gutter sign="+" n={row.b !== null ? row.b + 1 : null} />
        <code>{newLine !== null && (words ? <Spans spans={words.b} /> : newLine)}</code>
      </div>
    </div>
  )
}
