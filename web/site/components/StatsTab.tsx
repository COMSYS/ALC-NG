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
      <h3>Where the Original Bytes Went</h3>
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
      <div>
        <div className="section-label">Statistics</div>
        <h2>What the Cleaner Removed</h2>
      </div>
      <div className="stats-row">
        <div className="stat-card">
          <div className="stat-number">{fmtSize(stats.savedBytes)}</div>
          <div className="stat-label">removed in total</div>
        </div>
        <div className="stat-card">
          <div className="stat-number">
            {stats.origN} → {stats.outN}
          </div>
          <div className="stat-label">files before and after</div>
        </div>
        <div className="stat-card">
          <div className="stat-number">{stats.stats.U}</div>
          <div className="stat-label">files left unchanged</div>
        </div>
        <div className="stat-card">
          <div className="stat-number">{stats.archive ? fmtSize(stats.archive.size) : '—'}</div>
          <div className="stat-label">archive for upload</div>
        </div>
      </div>
      <div className="table-wrapper">
        <table className="table">
          <tbody>
            <tr>
              <td className="label">Size</td>
              <td>{sizeLine(stats)}</td>
            </tr>
            <tr>
              <td className="label">Files</td>
              <td>{filesLine(stats)}</td>
            </tr>
          </tbody>
        </table>
      </div>

      <Composition stats={stats} />

      <div className="tables">
        <section>
          <h3>By File Type</h3>
          <div className="table-wrapper">
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
          </div>
        </section>
        <section>
          <h3>Largest Reductions</h3>
          <div className="table-wrapper">
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
          </div>
        </section>
      </div>

      <p className="figure-caption">
        Sizes are measured on the {stats.outN} files written to {current.cfg.target}. A file that grew, such as a re-encoded image, counts as
        a reduction of zero.
      </p>
    </div>
  )
}
