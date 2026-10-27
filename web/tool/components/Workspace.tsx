import { useStore } from '../../shared/lib/store'
import { compareStore } from '../../shared/state/compare'
import { reviewStore } from '../../shared/state/review'
import { runStore } from '../../shared/state/steps'
import { showTab, uiStore, type Tab } from '../../shared/state/ui'
import { CompareTab } from './CompareTab'
import { DiffView } from '../../shared/components/DiffView'
import { PdfTab } from '../../shared/components/PdfTab'
import { ResultSummary } from './ResultSummary'
import { RunSteps } from './RunSteps'
import { StatsTab } from './StatsTab'

const TABS: [Tab, string][] = [
  ['review', 'Review'],
  ['pdf', 'PDF'],
  ['compare', 'Comparison'],
  ['stats', 'Statistics'],
]

export function Workspace() {
  const tab = useStore(uiStore, (s) => s.tab)
  const hasPdf = useStore(uiStore, (s) => s.pdf !== null)
  const hasCompare = useStore(compareStore, (s) => s.summary !== null && s.summary.state !== 'running')
  const hasReview = useStore(reviewStore, (s) => s.current !== null)
  const enabled: Record<Tab, boolean> = { review: true, pdf: hasPdf, compare: hasCompare, stats: hasReview }

  return (
    <>
      <div className="tabs" role="tablist">
        {TABS.map(([key, label]) => (
          <button
            key={key}
            type="button"
            role="tab"
            className={`tab${tab === key ? ' on' : ''}`}
            aria-selected={tab === key}
            disabled={!enabled[key]}
            onClick={() => showTab(key)}
          >
            {label}
          </button>
        ))}
      </div>
      {/* inactive panes stay mounted (the PDF iframe keeps its document) */}
      <div className="content" hidden={tab !== 'review'}>
        <ReviewPane />
      </div>
      <div className="content flush" hidden={tab !== 'pdf'}>
        <PdfTab />
      </div>
      <div className="content" hidden={tab !== 'compare'}>
        <CompareTab />
      </div>
      <div className="content" hidden={tab !== 'stats'}>
        <StatsTab />
      </div>
    </>
  )
}

function ReviewPane() {
  const view = useStore(reviewStore, (s) => s.view)
  const running = useStore(runStore, (s) => s.visible)
  if (running) return <RunSteps />
  if (view === 'done') return <ResultSummary />
  if (view === 'diff') return <DiffView />
  return (
    <div className="empty">
      <h2>No results yet</h2>
      <p>
        Choose the folder of a LaTeX submission, or load a sample, then run alc-ng. It compiles the sources, removes what the build does not
        use, and lists every changed file here with a line diff.
      </p>
    </div>
  )
}
