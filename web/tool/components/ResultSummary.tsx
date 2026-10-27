import { useMemo } from 'react'
import { fmtSize, plural } from '../../shared/lib/format'
import { baseOf } from '../../shared/lib/paths'
import { computeStats, downloadArchive, filesLine, firstChange, openDiff, sizeLine } from '../../shared/lib/review'
import { useStore } from '../../shared/lib/store'
import { reviewStore, type CompareOutcome } from '../../shared/state/review'
import { showTab, uiStore } from '../../shared/state/ui'

function CompareFact({ compare, mains, main }: { compare: CompareOutcome; mains: number; main: string }) {
  if (compare.state === 'ok') {
    return (
      <>
        <span className="badge ok">Identical</span> {compare.pages} {plural(compare.pages, 'page')} rendered pixel by pixel
        {mains > 1 && `, ${main}`}
      </>
    )
  }
  if (compare.state === 'differs') {
    return (
      <>
        <span className="badge bad">
          {compare.differing} of {compare.pages} pages differ
        </span>
        <button type="button" className="linkButton" onClick={() => showTab('compare')}>
          Show pages
        </button>
      </>
    )
  }
  return (
    <>
      <span className="badge bad">Not completed</span> {compare.text}. See the log.
    </>
  )
}

// What the review tab shows once a run has finished: the outcome in a few
// lines, the archive download, and the way into the diffs.
export function ResultSummary() {
  const current = useStore(reviewStore, (s) => s.current)
  const label = useStore(uiStore, (s) => s.folder?.label ?? 'submission')
  const stats = useMemo(() => current && computeStats(current.review, current.totalMs), [current])
  if (!current || !stats) return null
  const { review, compare, run } = current
  const first = firstChange(review)

  return (
    <section className="result">
      <h2>Cleaned {label}</h2>
      <dl className="facts">
        <dt>Size</dt>
        <dd>{sizeLine(stats)}</dd>
        <dt>Files</dt>
        <dd>{filesLine(stats)}</dd>
        {compare && (
          <>
            <dt>PDF</dt>
            <dd>
              <CompareFact compare={compare} mains={run.mains.length} main={run.main} />
            </dd>
          </>
        )}
        <dt>Time</dt>
        <dd>{(stats.totalMs / 1000).toFixed(1)} s</dd>
      </dl>
      <div className="actions">
        {review.archive && (
          <button type="button" className="button primary" onClick={downloadArchive}>
            Download {baseOf(review.archive.path)} ({fmtSize(review.archive.size)})
          </button>
        )}
        {first && (
          <button type="button" className="button" onClick={() => void openDiff(first)}>
            Review changes
          </button>
        )}
      </div>
      <p className="help">
        {review.archive
          ? 'Select a file under Changes to see its diff.'
          : 'No archive was written. Turn on “Create archive” to get a file ready for upload.'}
      </p>
    </section>
  )
}
