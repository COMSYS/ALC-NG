export function Header() {
  return (
    <header className="topbar">
      <h1 className="brandName">alc-ng</h1>
      <p className="brandDesc">Removes comments, unused files and other leftovers from LaTeX sources before an arXiv upload. Runs in this tab; nothing is uploaded.</p>
      <nav className="topnav">
        <a href="https://arxiv.comsys.rwth-aachen.de/" target="_blank" rel="noopener">
          Paper
        </a>
        <a href="https://github.com/COMSYS/ALC-NG" target="_blank" rel="noopener">
          Source code
        </a>
      </nav>
    </header>
  )
}
