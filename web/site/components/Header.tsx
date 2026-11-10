import alcng from '../assets/alcng-logo.svg'
import comsys from '../assets/rwth_comsys.svg'
import { ChartIcon, GitHubIcon, PdfFileIcon } from './icons'

// The header of the project page "Hidden Secrets in the arXiv", which this
// tool is part of, in short.
export function Header() {
  return (
    <header className="hero">
      {/* The tool's own mark first (logo.svg: the broom sweeping \alcng{}),
          the group it comes from second — the same lockup as the repository,
          so a visitor arriving from GitHub recognizes the page. */}
      <div className="hero-brands">
        <a className="hero-mark" href="https://github.com/COMSYS/ALC-NG" target="_blank" rel="noopener">
          <img src={alcng} alt="ALC-NG" />
        </a>
        <a className="hero-affil" href="https://www.comsys.rwth-aachen.de" target="_blank" rel="noopener">
          <img src={comsys} alt="COMSYS / RWTH Aachen University" />
        </a>
      </div>
      <div className="hero-badge">ALC-NG in the browser</div>
      <h1>
        Clean Your <span className="highlight">arXiv</span> Sources
      </h1>
      <p className="subtitle">
        Before you publish your LaTeX sources to arXiv, alc-ng strips out comments, unused files and other hidden content. The cleaner
        runs fully in your browser, so your secret paper contents remain secret!
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
