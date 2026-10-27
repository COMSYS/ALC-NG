import { fmtSize } from '../../shared/lib/format'
import { useStore } from '../../shared/lib/store'
import { compareStore, type CompareCard } from '../../shared/state/compare'

const CAPTIONS: Record<CompareCard['kind'], string> = {
  diff: 'changed pixels marked',
  'original only': 'only in the original',
  'cleaned only': 'only in the cleaned version',
}

// The pixel comparison of the original and the cleaned PDF: the verdict,
// then one image per page that is not identical.
export function CompareTab() {
  const { summary, cards } = useStore(compareStore, (s) => s)
  if (!summary) return null
  return (
    <>
      <div>
        <div className="section-label">Pixel comparison</div>
        <h2>Original and Cleaned PDF, Page by Page</h2>
      </div>
      {summary.state === 'running' && <p className="callout callout-info">Comparing…</p>}
      {summary.state === 'ok' && (
        <div className="callout callout-success">
          <strong>Pixel-perfect</strong>
          {summary.pages === 1 ? `The page of ${summary.main} renders` : `All ${summary.pages} pages of ${summary.main} render`} the same before
          and after cleaning ({summary.ms} ms).
        </div>
      )}
      {summary.state === 'differs' && (
        <div className="callout callout-warning">
          <strong>
            {summary.differing} of {summary.pages} pages differ
          </strong>
          Changed pixels are marked in the diff colour below ({summary.ms} ms).
        </div>
      )}
      {summary.state === 'failed' && (
        <div className="callout callout-error">
          <strong>Comparison failed</strong>
          {summary.message}
        </div>
      )}
      <div className="compareGrid">
        {cards.map((card) => (
          <figure key={card.index}>
            <a href={card.url} target="_blank" rel="noopener">
              <img src={card.url} alt={`Page ${card.index + 1}, ${CAPTIONS[card.kind]}`} />
            </a>
            <figcaption>
              <span>
                Page {card.index + 1}, {CAPTIONS[card.kind]}
              </span>
              <a href={card.url} download={card.name}>
                JPEG, {fmtSize(card.size)}
              </a>
            </figcaption>
          </figure>
        ))}
      </div>
    </>
  )
}
