import { ChangesPanel } from './components/ChangesPanel'
import { SubmissionPanel } from './components/SubmissionPanel'
import { Footer } from './components/Footer'
import { Header } from './components/Header'
import { LogPanel } from './components/LogPanel'
import { Workspace } from './components/Workspace'

export function App() {
  return (
    <>
      <Header />
      <main className="appMain">
        <div className="container-wide">
          <div className="appGrid">
            <aside className="appSide">
              <SubmissionPanel />
              <ChangesPanel />
            </aside>
            <Workspace />
          </div>
          <div className="appLog">
            <LogPanel />
          </div>
        </div>
      </main>
      <Footer />
    </>
  )
}
