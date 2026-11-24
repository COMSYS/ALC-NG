import { fmtSize, plural } from '../../shared/lib/format'
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
      <p className="compareHead">
        {summary.state === 'running' && <span className="muted">Comparing…</span>}
        {summary.state === 'ok' && (
          <>
            <span className="badge ok">Identical</span> All {summary.pages} {plural(summary.pages, 'page')} of {summary.main} render the same
            before and after cleaning. <span className="muted">({summary.ms} ms)</span>
          </>
        )}
        {summary.state === 'differs' && (
          <>
            <span className="badge bad">
              {summary.differing} of {summary.pages} pages differ
            </span>{' '}
            in {summary.main}. <span className="muted">({summary.ms} ms)</span>
          </>
        )}
        {summary.state === 'failed' && (
          <>
            <span className="badge bad">Failed</span> {summary.message}
          </>
        )}
      </p>
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
