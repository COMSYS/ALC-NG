# TeXlyre BusyTeX engines (pdfTeX, XeTeX, TeX Live 2026)

A second engine build next to the SwiftLaTeX one (`../swiftlatex/`), selected
on the page by **Engine build**. Nothing here is committed except this file
and `alc-texlyre-worker.js`:

```sh
sh wasm-test/fetch-texlyre.sh        # BIBER=1 adds the biber module
```

downloads `busytex.{js,wasm}`, `busytex_pipeline.js`, `busytex_biber.js` and
`texlive-basic.{js,data}` from a pinned build of
[texlyre-busytex-build](https://github.com/TeXlyre/texlyre-busytex-build)
(sources: [TeXlyre/busytex](https://github.com/TeXlyre/busytex), AGPL-3.0).

## How it plugs in

`alc-texlyre-worker.js?engine=pdftex|xetex` answers the worker protocol of the
patched SwiftLaTeX engines (`web/shared/lib/engine.ts`) on top of BusyTeX's
`BusytexPipeline`, so the page, the cleaner sidecar (`../swiftlatex/alc-wasi.js`,
now running on BusyTeX's Emscripten FS with `/work` as the project directory)
and the review code are shared. Differences, reflected in the engine specs:

- `compilelatex` is the whole run: TeX passes (the pipeline reruns up to 3
  times on the log's rerun hints), BibTeX or biber, makeindex, and for XeTeX
  xdvipdfmx — the reply carries a PDF, there is no separate dvipdfmx worker.
- The LaTeX formats ship prebuilt in `texlive-basic`; nothing is built or
  seeded.
- The access log is recorded at `FS.open` while an applet runs (the
  `-recorder` file never appears in this build), so BibTeX and xdvipdfmx opens
  are covered as with the SwiftLaTeX syscall hook.
- The dev server answers a missing TeX Live file with a 404 for this worker
  (`?miss=404`), which BusyTeX remembers; the SwiftLaTeX workers keep getting
  their 301 marker.
- TeX Live files beyond `texlive-basic` are fetched by the module with
  synchronous XHRs from `/texlive/<engine>/<format>/<name>` — the route the
  dev server already has for the SwiftLaTeX workers.
- Fonts are found by kpathsea only, as with SwiftLaTeX: `\setmainfont{…}` by
  file name works, system font names do not (BusyTeX's font index is not
  served here).
