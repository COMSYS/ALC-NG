import { useMemo } from 'react'
import { fmtSize, plural } from '../../shared/lib/format'
import { baseOf } from '../../shared/lib/paths'
import { computeStats, downloadArchive, filesLine, firstChange, openDiff, sizeLine, type Stats } from '../../shared/lib/review'
import { useStore } from '../../shared/lib/store'
import { reviewStore, type CompareOutcome } from '../../shared/state/review'
import { showTab, uiStore } from '../../shared/state/ui'

const percentSmaller = (stats: Stats) => {
  const pct = stats.origBytes > 0 ? (stats.savedBytes / stats.origBytes) * 100 : 0
  return `${pct.toFixed(pct < 10 ? 1 : 0)}%`
}

function CompareCallout({ compare, mains, main }: { compare: CompareOutcome; mains: number; main: string }) {
  if (compare.state === 'ok') {
    return (
      <div className="callout callout-success">
        <strong>Pixel-perfect</strong>
        The cleaned PDF renders identically to the original, {compare.pages === 1 ? 'on its only page' : `on all ${compare.pages} pages`}
        {mains > 1 && ` (compared: ${main})`}.
      </div>
    )
  }
  if (compare.state === 'differs') {
    return (
      <div className="callout callout-warning">
        <strong>
          {compare.differing} of {compare.pages} pages differ
        </strong>
        The cleaned PDF does not render exactly like the original.
        <button type="button" className="linkButton" onClick={() => showTab('compare')}>
          Show the pages
        </button>
      </div>
    )
  }
  return (
    <div className="callout callout-error">
      <strong>Comparison not completed</strong>
      {compare.text.charAt(0).toUpperCase() + compare.text.slice(1)}. See the processing log for details.
    </div>
  )
}

// What the review tab shows once a run has finished: the headline numbers,
// the PDF comparison, the archive download and the way into the diffs.
export function ResultSummary() {
  const current = useStore(reviewStore, (s) => s.current)
  const label = useStore(uiStore, (s) => s.folder?.label ?? 'submission')
  const stats = useMemo(() => current && computeStats(current.review, current.totalMs), [current])
  if (!current || !stats) return null
  const { review, compare, run } = current
  const first = firstChange(review)
  const s = stats.stats

  return (
    <section className="result">
      <div className="section-label">Result</div>
      <h2>Cleaned {label}</h2>
      <div className="stats-row">
        <div className="stat-card">
          <div className="stat-number">{percentSmaller(stats)}</div>
          <div className="stat-label">smaller than the original sources</div>
        </div>
        <div className="stat-card">
          <div className="stat-number">{s.D}</div>
          <div className="stat-label">{plural(s.D, 'file')} removed</div>
        </div>
        <div className="stat-card">
          <div className="stat-number">{s.M}</div>
          <div className="stat-label">{plural(s.M, 'file')} cleaned</div>
        </div>
        <div className="stat-card">
          <div className="stat-number">{(stats.totalMs / 1000).toFixed(1)}s</div>
          <div className="stat-label">to compile and clean</div>
        </div>
      </div>
      {compare && <CompareCallout compare={compare} mains={run.mains.length} main={run.main} />}
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
            <tr>
              <td className="label">Archive</td>
              <td>{review.archive ? `${baseOf(review.archive.path)}, ${fmtSize(review.archive.size)}` : 'not written'}</td>
            </tr>
          </tbody>
        </table>
      </div>
      <div className="actions">
        {review.archive && (
          <button type="button" className="button primary" onClick={downloadArchive}>
            Download {baseOf(review.archive.path)}
          </button>
        )}
        {first && (
          <button type="button" className="button" onClick={() => void openDiff(first)}>
            Review the changes
          </button>
        )}
      </div>
      <p className="help">
        {review.archive
          ? 'The archive is ready to upload to arXiv. Select a file under Changes to see what was removed from it.'
          : 'No archive was written. Turn on “Create archive” in the options to get a file ready for upload.'}
      </p>
    </section>
  )
}
