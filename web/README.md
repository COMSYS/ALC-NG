# alc-ng web

The browser front end of alc-ng: a Vite + React + TypeScript app that cleans
an arXiv submission entirely in the browser. It compiles the submission with
the SwiftLaTeX engines (pdfTeX, or XeTeX plus dvipdfmx), runs the real alc-ng
pipeline built for `wasm32-wasip1` inside the engine worker, and shows the
result as a file-by-file review, the compiled PDF, a pixel comparison of the
original and the cleaned PDF, and statistics.

The app exists in two designs, side by side: `tool/` is a plain working tool
(form on the left, result on the right, log underneath), `site/` takes the
look of the project page arxiv.comsys.rwth-aachen.de. Both run the same code
from `shared/`; `index.html` at the root links to the two.

Everything besides the app bundle comes from the axum server in
`wasm-test/server`: the TeX Live proxy (`/texlive/`), prebuilt format dumps
(`/fmt/`), folder listings (`/tree/`), the patched engine workers and pdfium
(`/wasm-test/`), the `alc_ng.wasm` build (`/target/`) and the bundled samples.

## Prerequisites

- the alc-ng cdylib: `cargo build --target wasm32-wasip1 --lib --release`
  (see `.github/workflows/wasm.yml` for the wasi-sdk setup)
- the patched SwiftLaTeX engines in `wasm-test/swiftlatex/` and pdfium in
  `wasm-test/pdfium/` (see the READMEs there)
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

The designs are at http://localhost:5173/tool/ and http://localhost:5173/site/.

`ALC_SERVER=http://host:port pnpm dev` points the dev server at a backend on
another address. `VITE_ALC_NG_WASM=<url>` loads `alc_ng.wasm` from somewhere
other than `/target/wasm32-wasip1/release/alc_ng.wasm`.

Query parameters for testing: `?sample=<index>` loads a bundled sample on
start, `?preload=1` starts the engine worker right away, and `?wasitrace=1`
traces the WASI calls of the alc-ng module.

The PDF tab renders with pdf.js (`pdfjs-dist`), so it does not depend on the
browser's own PDF viewer. Its worker is bundled; its CMaps, standard fonts and
image decoders are copied into the build under `assets/pdfjs-<version>/`
(see `vite.config.ts`).

## Production

```sh
pnpm --dir web build   # writes web/dist
cargo run --release --manifest-path wasm-test/server/Cargo.toml
```

The server serves the overview `web/dist/index.html` at `/`, the designs at
`/tool/` and `/site/`, and the hashed bundles of both under `/assets/`
(`SERVE_WEB` points it at another build). `Dockerfile.wasm` builds
the app in its own stage and ships it with the server.

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
- `shared/start.tsx` — mounts a design's app and reads the query parameters
- `tool/`, `site/` — one design each: `index.html`, `main.tsx`, `App.tsx`,
  `index.css` and the components whose markup differs between the designs
