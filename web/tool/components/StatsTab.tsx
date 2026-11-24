import { useMemo } from 'react'
import { fmtSize } from '../../shared/lib/format'
import { computeStats, filesLine, openDiff, sizeLine, type Stats } from '../../shared/lib/review'
import { useStore } from '../../shared/lib/store'
import { reviewStore } from '../../shared/state/review'

const signed = (bytes: number) => (bytes > 0 ? `−${fmtSize(bytes)}` : bytes < 0 ? `+${fmtSize(-bytes)}` : '0 B')

// Where the bytes of the original submission went: survived untouched,
// trimmed out of kept files, dropped with whole files, or added.
function Composition({ stats }: { stats: Stats }) {
  const survived = Math.max(0, stats.origBytes - stats.droppedBytes - stats.trimmedBytes)
  const parts: [string, number, string][] = [
    ['c-keep', survived, 'unchanged'],
    ['c-trim', stats.trimmedBytes, 'removed from kept files'],
    ['c-drop', stats.droppedBytes, 'in removed files'],
    ['c-add', stats.addedBytes, 'added'],
  ]
  return (
    <section>
      <h3>Original size, by outcome</h3>
      <div className="composition" role="img" aria-label={parts.map(([, b, l]) => `${fmtSize(b)} ${l}`).join(', ')}>
        {parts
          .filter(([, bytes]) => bytes > 0)
          .map(([cls, bytes, label]) => (
            <span key={cls} className={cls} style={{ flex: `${bytes} 1 0` }} title={`${fmtSize(bytes)} ${label}`} />
          ))}
      </div>
      <div className="legend">
        {parts
          .filter(([cls, bytes]) => cls !== 'c-add' || bytes > 0)
          .map(([cls, bytes, label]) => (
            <span key={cls} className={cls}>
              <i />
              {fmtSize(bytes)} {label}
            </span>
          ))}
      </div>
    </section>
  )
}

export function StatsTab() {
  const current = useStore(reviewStore, (s) => s.current)
  const stats = useMemo(() => current && computeStats(current.review, current.totalMs), [current])
  if (!current || !stats) return null

  const open = (rel: string) => {
    const change = current.review.changes.find((c) => c.rel === rel) ?? current.review.unchanged.find((c) => c.rel === rel)
    if (change) void openDiff(change)
  }
  const top = stats.reductions.slice(0, 10)

  return (
    <div className="stats">
      <dl className="facts">
        <dt>Size</dt>
        <dd>{sizeLine(stats)}</dd>
        <dt>Files</dt>
        <dd>
          {filesLine(stats)} ({stats.origN} before, {stats.outN} after)
        </dd>
        <dt>Archive</dt>
        <dd>{stats.archive ? fmtSize(stats.archive.size) : 'not written'}</dd>
        <dt>Time</dt>
        <dd>{(stats.totalMs / 1000).toFixed(1)} s</dd>
      </dl>

      <Composition stats={stats} />

      <div className="tables">
        <section>
          <h3>By file type</h3>
          <table className="table">
            <thead>
              <tr>
                <th>Type</th>
                <th className="num">Files</th>
                <th className="num">Before</th>
                <th className="num">After</th>
                <th className="num">Change</th>
              </tr>
            </thead>
            <tbody>
              {stats.kindRows.map((k) => (
                <tr key={k.kind}>
                  <td>{k.kind}</td>
                  <td className="num">
                    {k.origN} → {k.outN}
                  </td>
                  <td className="num">{fmtSize(k.origB)}</td>
                  <td className="num">{fmtSize(k.outB)}</td>
                  <td className="num">{signed(k.removed)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
        <section>
          <h3>Largest reductions</h3>
          <table className="table">
            <thead>
              <tr>
                <th>File</th>
                <th className="num">Saved</th>
              </tr>
            </thead>
            <tbody>
              {top.length === 0 && (
                <tr className="none">
                  <td colSpan={2}>No file got smaller.</td>
                </tr>
              )}
              {top.map((r) => (
                <tr key={r.rel}>
                  <td className="file">
                    <span className={`st st-${r.status}`}>{r.status}</span>{' '}
                    <button type="button" className="linkButton" onClick={() => open(r.rel)}>
                      {r.rel}
                    </button>
                  </td>
                  <td className="num">{fmtSize(r.saved)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      </div>

      <p className="help">
        Sizes are measured on the {stats.outN} files written to {current.cfg.target}. A file that grew, such as a re-encoded image, counts as
        a reduction of zero.
      </p>
    </div>
  )
}
