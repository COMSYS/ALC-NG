# alc-ng web

The browser front end of alc-ng: a Vite + React + TypeScript app that cleans
an arXiv submission entirely in the browser. It compiles the submission with
the SwiftLaTeX engines (pdfTeX, or XeTeX plus dvipdfmx), runs the real alc-ng
pipeline built for `wasm32-wasip1` inside the engine worker, and shows the
result as a file-by-file review, the compiled PDF, a pixel comparison of the
original and the cleaned PDF, and statistics.

A submission comes in as a folder, as the archive it is held in (`.zip`,
`.tar`, `.tar.gz`, or a single gzipped file — `shared/lib/archive.ts`, no
dependency, `DecompressionStream` does the work), as an arXiv identifier
(`shared/lib/arxiv.ts` downloads the sources from export.arxiv.org, which
allows the cross-origin read, so the bytes never pass through this server),
or as one of the bundled samples.

The app exists in two designs: `site/` is the default — the app in the look
of the project page arxiv.comsys.rwth-aachen.de, served at the root — and
`tool/` is a plain working tool (form on the left, result on the right, log
underneath) kept under `/tool/`. Both run the same code from `shared/`.

Everything besides the app bundle comes from the axum server in
`wasm-test/server`: the TeX Live proxy (`/texlive/`), prebuilt format dumps
(`/fmt/`), folder listings (`/tree/`), the patched engine workers and pdfium
(`/wasm-test/`), the `alc_ng.wasm` build (`/target/`) and the bundled samples.

## Prerequisites

- the alc-ng cdylib: `cargo build --target wasm32-wasip1 --lib --release`
  (see `.github/workflows/wasm.yml` for the wasi-sdk setup)
- the patched SwiftLaTeX engines in `wasm-test/swiftlatex/` and pdfium in
  `wasm-test/pdfium/` (see the READMEs there)
- optionally the TeXlyre BusyTeX engines (TeX Live 2026) in
  `wasm-test/texlyre/`, `sh wasm-test/fetch-texlyre.sh`; the page's **Engine
  build** option picks between the two builds
- a local TeX Live with `kpsewhich`, which the server resolves files with
- Node.js and pnpm (the version is pinned in `package.json`)

## Development

```sh
# terminal 1, from the repository root: the backend on :8000
cargo run --release --manifest-path wasm-test/server/Cargo.toml

# terminal 2: the app with hot reload on :5173, forwarding the backend routes
pnpm --dir web install
pnpm --dir web dev
```

The app is at http://localhost:5173/ (`/site/` forwards there), the tool
design at http://localhost:5173/tool/.

`ALC_SERVER=http://host:port pnpm dev` points the dev server at a backend on
another address. `VITE_ALC_NG_WASM=<url>` loads `alc_ng.wasm` from somewhere
other than `/target/wasm32-wasip1/release/alc_ng.wasm`. `VITE_SITE_URL`
(`.env`, `https://alc.ng/`) is the address the build is deployed at: it
fills the canonical link and the og:/twitter: tags of `site/index.html`,
which need absolute URLs for a shared link to unfurl with its card.

Query parameters for testing: `?sample=<index>` loads a bundled sample on
start, `?preload=1` starts the engine worker right away, and `?wasitrace=1`
traces the WASI calls of the alc-ng module.

The samples the picker offers (`shared/lib/samples.ts`) are folders of this
repository, so they work in every deployment; `Dockerfile.wasm` copies
exactly those and fails the build if one is missing. No real paper is
bundled — the arXiv field fetches any of them on demand.

The PDF tab renders with pdf.js (`pdfjs-dist`), so it does not depend on the
browser's own PDF viewer. Its worker is bundled; its CMaps, standard fonts and
image decoders are copied into the build under `assets/pdfjs-<version>/`
(see `vite.config.ts`).

## Production

```sh
pnpm --dir web build   # writes web/dist
cargo run --release --manifest-path wasm-test/server/Cargo.toml
```

The server serves `web/dist/site/index.html` at `/` (`/site/` forwards
there), the tool design at `/tool/`, and the hashed bundles of both under
`/assets/` (`SERVE_WEB` points it at another build). `Dockerfile.wasm`
builds the app in its own stage and ships it with the server.

## Checks

```sh
pnpm --dir web typecheck
pnpm --dir web lint
pnpm --dir web test
```

## Layout

- `shared/lib/` — the pipeline, framework-free: the engine worker protocol
  (`engine.ts`), format dumps (`fmt.ts`), compiling and the recorder file
  (`compile.ts`), dvipdfmx (`dvipdfm.ts`), the pixel compare (`pdfium.ts`,
  `compare.ts`), the review and statistics (`review.ts`), the line diff
  (`diff.ts`) and the orchestration of a run (`pipeline.ts`)
- `shared/state/` — small external stores the pipeline reports into (UI state,
  run progress, review, compare, processing log)
- `shared/components/` — the views both designs use unchanged: the diff, the
  pdf.js viewer, the folder drop zone and the sample picker (which take their
  icon from the design)
- `public/assets/` — the files a build copies verbatim into `dist/assets/`:
  the ALC-NG mark for the tab icon and the link-preview card. Everything
  under `/assets/` is served immutably, so these names carry a version
  (`-v1`) that a replacement has to bump
- `shared/start.tsx` — mounts a design's app and reads the query parameters
- `site/`, `tool/` — one design each: `index.html`, `main.tsx`, `App.tsx`,
  `index.css` and the components whose markup differs between the designs
  (`site/` is the one served at the root)
