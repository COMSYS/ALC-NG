import { useEffect, useState } from 'react'
import { fmtDur, plural } from '../../shared/lib/format'
import { useStore } from '../../shared/lib/store'
import { RUN_STEPS, runStore, type StepKey, type StepRow } from '../../shared/state/steps'
import { CheckIcon, CrossIcon, DashIcon, DotIcon } from './icons'

const STEP_NAMES: Record<StepKey, string> = {
  locate: 'Main file',
  engine: 'TeX engine',
  stage: 'Stage files',
  compile: 'Compile',
  recorder: 'Recorder file',
  cleaner: 'Clean',
  review: 'Compare trees',
  compare: 'Compare PDFs',
}

// The run as a list of its steps, each with its state, latest detail and
// duration. A failed run keeps the list up, marking where it stopped.
export function RunSteps() {
  const run = useStore(runStore, (s) => s)
  const rows = RUN_STEPS.map(([key]) => run.rows[key])
  const resolved = rows.filter((r) => r.state === 'done' || r.state === 'skipped' || r.state === 'failed').length
  // the clock runs until the run has finished or failed
  const ticking = run.visible && run.failure === null && resolved < rows.length
  const [now, setNow] = useState(() => performance.now())

  useEffect(() => {
    if (!ticking) return
    const tick = () => setNow(performance.now())
    tick()
    const timer = window.setInterval(tick, 200)
    return () => window.clearInterval(timer)
  }, [ticking])

  // a stopped clock shows the end of the last settled step
  const end = ticking ? now : Math.max(run.t0, ...rows.filter((r) => r.ms !== null).map((r) => r.t0 + (r.ms ?? 0)))
  const heading = run.failure !== null ? `Run on ${run.label} failed` : resolved < rows.length ? `Running on ${run.label}` : `Finished ${run.label}`

  return (
    <section className="run" aria-live="polite">
      <div className="runHead">
        <h2>{heading}</h2>
        <span className="elapsed">{((end - run.t0) / 1000).toFixed(1)} s</span>
      </div>
      <div className="runBar">
        <i style={{ width: `${(resolved / rows.length) * 100}%` }} />
      </div>
      <ol className="steps">
        {RUN_STEPS.map(([key]) => (
          <Step key={key} name={STEP_NAMES[key]} row={run.rows[key]} now={now} />
        ))}
      </ol>
      {run.failure !== null && (
        <p className="runError">
          {run.failure}
          {!/log/i.test(run.failure) && ' See the log below for details.'}
        </p>
      )}
    </section>
  )
}

function StepIcon({ state }: { state: StepRow['state'] }) {
  if (state === 'done') return <CheckIcon />
  if (state === 'failed') return <CrossIcon />
  if (state === 'skipped') return <DashIcon />
  if (state === 'running') return <span className="spinner" />
  return <DotIcon />
}

function Step({ name, row, now }: { name: string; row: StepRow; now: number }) {
  const time = row.state === 'skipped' ? 'skipped' : row.state === 'running' ? fmtDur(now - row.t0) : row.ms !== null ? fmtDur(row.ms) : ''
  const details = [row.sub]
  if (row.downloads > 0) details.push(`${row.downloads} TeX Live ${plural(row.downloads, 'file')}`)
  return (
    <li className={`step ${row.state}`}>
      <span className="stepIcon">
        <StepIcon state={row.state} />
      </span>
      <span className="stepName">{name}</span>
      <span className="stepDetail">{details.filter(Boolean).join('; ')}</span>
      <span className="stepTime">{time}</span>
    </li>
  )
}
