// The footer of the project page, with the template credit its licence asks for.
export function Footer() {
  return (
    <footer>
      <div className="footer-inner">
        <p className="footer-line">
          Part of <a href="https://arxiv.comsys.rwth-aachen.de/">Hidden Secrets in the arXiv</a> (IEEE S&amp;P 2026). The command-line
          version of ALC-NG also strips metadata and works offline; see{' '}
          <a href="https://github.com/COMSYS/ALC-NG" target="_blank" rel="noopener">
            GitHub
          </a>
          .
        </p>
        <div className="footer-divider" />
        <p className="footer-legal">
          Design after the template by{' '}
          <a href="https://seemandhar.github.io/" target="_blank" rel="noopener">
            Seemandhar Jain
          </a>
          , licensed under{' '}
          <a href="http://creativecommons.org/licenses/by-sa/4.0/" target="_blank" rel="noopener">
            CC BY-SA 4.0
          </a>
          .
        </p>
        <p className="footer-legal">
          <a href="https://www.comsys.rwth-aachen.de/impressum/" target="_blank" rel="noopener">
            Impressum
          </a>
          &nbsp;·&nbsp;
          <a href="https://www.comsys.rwth-aachen.de/datenschutz/" target="_blank" rel="noopener">
            Datenschutz
          </a>
        </p>
      </div>
    </footer>
  )
}
