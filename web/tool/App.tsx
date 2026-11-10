import { ChangesPanel } from './components/ChangesPanel'
import { SubmissionPanel } from './components/SubmissionPanel'
import { Header } from './components/Header'
import { LogPanel } from './components/LogPanel'
import { Workspace } from './components/Workspace'

export function App() {
  return (
    <div className="app">
      <Header />
      <div className="body">
        <aside className="sidebar">
          <SubmissionPanel />
          <ChangesPanel />
        </aside>
        <main className="main">
          <Workspace />
          <LogPanel />
        </main>
      </div>
    </div>
  )
}
