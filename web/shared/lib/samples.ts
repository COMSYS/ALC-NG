// Bundled samples: a comment-removal fixture and one real arXiv submission
// (Bubeck et al.'s “Sparks of AGI”, under wasm-test/fixtures/ and kept out of
// git, like any real paper content).
//
// A sample names its folder (`dir`); it either lists its files (`files`,
// fetched as <dir>/<file>) or lets the server list the folder with
// GET /tree/<dir> — so a fresh submission only needs to be unpacked under
// wasm-test/fixtures/ and named here. `pick`/`blurb` are what the sample
// picker in the sidebar shows.
//
// fixtures/bubeck is arXiv-2303.12712v5 unpacked, with one repair: its
// contents/3_code.tex includes figures/DV3_plot_{1,2,3}.png, which on disk are
// dv3_plot_{1,2,3}.png. macOS resolves that, the engine's case-sensitive FS
// (like arXiv's Linux) does not, so the three references were lower-cased.
// The v5 minted cache (minted-cache/, frozen via frozencache=true) covers all
// 29 minted chunks of this version, so the engine compiles it without Python
// or shell-escape.
export interface Sample {
  label: string
  pick: string
  blurb: string
  dir: string
  files?: string[]
}

export const SAMPLES: Sample[] = [
  { label: 'bubeck', pick: 'bubeck', blurb: 'Sparks of AGI, 118 files (arXiv 2303.12712v5)', dir: '/wasm-test/fixtures/bubeck' },
  {
    label: 'tests/appendix',
    pick: 'appendix',
    blurb: 'comment-removal cases, paper.tex + example.bib',
    dir: '/tests/appendix',
    files: ['paper.tex', 'example.bib'],
  },
]
