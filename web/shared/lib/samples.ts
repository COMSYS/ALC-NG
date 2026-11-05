// Bundled samples: small submissions kept in the repository, so the picker
// works in every deployment (the Docker image copies exactly these — see
// Dockerfile.wasm, which fails the build when one of them is missing).
//
// A sample names its folder (`dir`); it either lists its files (`files`,
// fetched as <dir>/<file>) or lets the server list the folder with
// GET /tree/<dir>. `pick`/`blurb` are what the sample picker shows: say what
// the sample demonstrates, not what it is called on disk.
//
// Real papers are deliberately not bundled. Someone else's submission shown
// as an example of hidden content is a pillory, and the arXiv field fetches
// any paper on demand anyway.
export interface Sample {
  label: string
  pick: string
  blurb: string
  dir: string
  files?: string[]
}

export const SAMPLES: Sample[] = [
  {
    label: 'comments',
    pick: 'Comments and hidden text',
    blurb: '2 files, pdfLaTeX — comments, dropped \\if branches, text after \\end{document}',
    dir: '/tests/appendix',
    files: ['paper.tex', 'example.bib'],
  },
  {
    label: 'nested',
    pick: 'Unused files in subfolders',
    blurb: '11 files, pdfLaTeX — editor backups, logs and an include nothing uses',
    dir: '/wasm-test/sample-nested',
  },
  {
    label: 'xelatex',
    pick: 'Figures and Unicode (XeLaTeX)',
    blurb: '5 files, XeLaTeX — an unused figure and an unused input; slower first run',
    dir: '/wasm-test/sample-xelatex',
  },
]
