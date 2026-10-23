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
    <section className="workspace">
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
    </section>
  )
}

function ReviewPane() {
  const view = useStore(reviewStore, (s) => s.view)
  const running = useStore(runStore, (s) => s.visible)
  if (running) return <RunSteps />
  if (view === 'done') return <ResultSummary />
  if (view === 'diff') return <DiffView />
  return (
    <div className="intro">
      <div className="section-label">Get started</div>
      <h2>Clean a Submission Before You Upload It</h2>
      <p className="secondary">
        Choose the folder of your LaTeX sources, or load a sample, and run ALC-NG. It compiles the sources here in your browser, removes what
        the build does not need, and lists every changed file with a line diff, the compiled PDF and a pixel comparison of the result.
      </p>
      <div className="accent-cards">
        <div className="accent-card">
          <h4>Dangling files</h4>
          <p>Files the compile never opens are left out, based on the engine&rsquo;s record of every file it read.</p>
        </div>
        <div className="accent-card">
          <h4>Irrelevant content</h4>
          <p>Comments, text after \end{'{document}'}, untaken \if branches and ignored command arguments are removed.</p>
        </div>
        <div className="accent-card muted">
          <h4>Embedded metadata</h4>
          <p>Not removed in the browser, since it needs exiftool. Use the command-line tool with --strip-exif for that.</p>
        </div>
      </div>
    </div>
  )
}
