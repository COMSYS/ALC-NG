import { useLayoutEffect, useRef, useState } from 'react'
import { useStore } from '../../shared/lib/store'
import { clearLog, logStore } from '../../shared/state/log'
import { ChevronIcon } from './icons'

// The processing log under the workspace. Collapsed, its head shows the
// newest line; open, it follows new output like a terminal, but only while
// the reader is at the bottom.
export function LogPanel() {
  const lines = useStore(logStore, (s) => s.lines)
  const [open, setOpen] = useState(false)
  const pane = useRef<HTMLPreElement>(null)
  const follow = useRef(true)

  useLayoutEffect(() => {
    const el = pane.current
    if (el && follow.current) el.scrollTop = el.scrollHeight
  }, [lines, open])

  const last = lines[lines.length - 1]
  return (
    <section className="log">
      <div className="logHead">
        <button
          type="button"
          className="sectionToggle"
          aria-expanded={open}
          aria-controls="logPane"
          onClick={() => {
            follow.current = true
            setOpen(!open)
          }}
        >
          <ChevronIcon />
          Log
        </button>
        {!open && <span className="logLast">{last?.text ?? ''}</span>}
        <button type="button" className="linkButton" onClick={clearLog}>
          Clear
        </button>
      </div>
      {open && (
        <pre
          id="logPane"
          ref={pane}
          className="logPane"
          aria-live="polite"
          onScroll={(ev) => {
            const el = ev.currentTarget
            follow.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24
          }}
        >
          {lines.map((line) => (
            <div key={line.id} className={`logline ${line.level}`}>
              <span className="time">{line.time}</span> {line.text}
            </div>
          ))}
        </pre>
      )}
    </section>
  )
}
