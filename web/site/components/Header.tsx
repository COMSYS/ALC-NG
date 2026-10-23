import logo from '../assets/rwth_comsys.svg'
import { ChartIcon, GitHubIcon, PdfFileIcon } from './icons'

// The header of the project page "Hidden Secrets in the arXiv", which this
// tool is part of, in short.
export function Header() {
  return (
    <header className="hero">
      <div className="hero-logo">
        <a href="https://www.comsys.rwth-aachen.de" target="_blank" rel="noopener">
          <img src={logo} alt="COMSYS / RWTH Aachen University" />
        </a>
      </div>
      <div className="hero-badge">ALC-NG in the browser</div>
      <h1>
        Clean Your <span className="highlight">arXiv</span> Sources
      </h1>
      <p className="subtitle">
        Remove comments, unused files and other hidden content from your LaTeX sources before you upload them. Everything runs in this
        browser tab; your files are never uploaded.
      </p>
      <div className="btn-row">
        <a className="btn" href="https://arxiv.comsys.rwth-aachen.de/">
          <span className="icon">
            <ChartIcon />
          </span>
          <span>Hidden Secrets in the arXiv</span>
        </a>
        <a
          className="btn"
          href="https://www.comsys.rwth-aachen.de/publication/2026/2026_pennekamp_hidden-secrets-in-the-arxiv/2026_pennekamp_hidden-secrets-in-the-arxiv.pdf"
          target="_blank"
          rel="noopener"
        >
          <span className="icon">
            <PdfFileIcon />
          </span>
          <span>Paper</span>
        </a>
        <a className="btn" href="https://github.com/COMSYS/ALC-NG" target="_blank" rel="noopener">
          <span className="icon">
            <GitHubIcon />
          </span>
          <span>ALC-NG Tool @ GitHub</span>
        </a>
      </div>
    </header>
  )
}
