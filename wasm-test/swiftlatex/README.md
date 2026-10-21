# Vendored SwiftLaTeX engines (pdfTeX, XeTeX, dvipdfmx)

These files are **not committed** (see `.gitignore`). Download them from the
[SwiftLaTeX releases](https://github.com/SwiftLaTeX/SwiftLaTeX/releases) (v20022022)
and extract here, keeping only:

- `swiftlatexpdftex.js` — the pdfTeX worker glue (**patched**, see below)
- `swiftlatexpdftex.wasm` — the pdfTeX WebAssembly engine (EPL-2.0 OR GPL-2.0 w/ Classpath-exception)
- `swiftlatexxetex.js` / `swiftlatexxetex.wasm` — the XeTeX engine (glue **patched**)
- `swiftlatexdvipdfm.js` / `swiftlatexdvipdfm.wasm` — dvipdfmx (glue **patched**)

`alc-wasi.js` is the exception: it is alc-ng's own code and **is tracked**. Both
TeX glues start with `importScripts("alc-wasi.js")`, so a checkout that has the
workers but not the shim fails on every `alcng_*` command (and the worker on
`ImportWorkerError`).

`python3 -I wasm-test/patch-swiftlatex.py <extracted release> wasm-test/swiftlatex`
writes **all three** patched workers from the extracted glue: patches 1–6 on
pdfTeX, patches 1–5 on XeTeX, and on dvipdfmx only the console forwarding and
the idempotent `mkdir`. Nothing is patched by hand anymore — the workers here
are `release + script`, byte for byte. Together with the `-cd` patch on all
three workers: `setmainfile` takes an optional `dir`
(relative to `/work`), and the compile runs and reads its output there, like
`latexmk -cd` — a nested main file such as `report/report.tex` that inputs
`../figs/…` compiles as it does natively.

`compilelatex` is one TeX pass followed by BibTeX. The page reruns it until
the `.aux` and `.bbl` stop changing (at most 5 passes, latexmk's
`max_repeat`), so citations and references resolve and the `.bbl` is read
back — native alc-ng keeps a `.bbl` the final pass reads.

**XeTeX** speaks the same worker protocol, but `compilelatex` hands back an
**XDV** in the reply's `pdf` field. The page then stages the submission plus
the XDV in a separate dvipdfmx worker (`compilepdf`) to get the PDF. XeTeX
writes the cache path of every OpenType font it loaded (`/tex/<fileid>`) into
the XDV; dvipdfmx asks for `xetex/<format>/<fileid>.otf`, which `serve.py`
maps back through `texlive-cache/fileids/`. Fonts are found by kpathsea only
(no fontconfig): `\setmainfont{texgyreheros-regular.otf}` works, system font
names do not. The page picks the engine per run (the `engine` flag; `auto`
reads a top-level `00README` and looks for fontspec, unicode-math,
polyglossia, xeCJK, …).

`PdfTeXEngine.js` is no longer used by the test page; the page drives the
worker protocol directly (see `index.html`, note in `wasm-test/serve.py`).

**The patches** — invented on `swiftlatexpdftex.js`, which is why they are
numbered from there; `patch-swiftlatex.py` applies them by anchor, so it fails
loudly if a future release moves the code it patches:

1. `seedfiles` command — the host can inject files (`{name: Uint8Array}`) that
   the engine's kpathsea hook serves before anything is fetched remotely.
2. `listdir` / `readfile` commands — the host can inspect `/work` after a run
   (used to recover the format dump, see below).
3. kpathsea hook consults seeded files first.
4. **File access log** — every `open` syscall (`___syscall_open`) records the
   path and access mode into `self.openLog`; the `openlog` command drains
   it to the host. The test page builds its used-file list from this log
   (read opens = inputs, write-only opens = outputs) instead of
   synthesizing a `.fls` from the compile log.
5. **alc-ng sidecar (WASI shim)** — `importScripts("alc-wasi.js")` at the top
   of the file loads a WASI preview1 shim that runs the **alc_ng wasm cdylib**
   (wasm32-wasip1) inside this worker against its Emscripten filesystem, so
   the real Rust pipeline (compile artifacts in, cleaned submission out)
   shares the engine's memfs. One dispatch branch routes `alcng_*` commands
   (`alcng_load` / `alcng_run` / `alcng_list` / `alcng_read` / `alcng_rm`) to
   it. The shim maps preopens (fd 3 = `/`, fd 4 = the cwd, reported with its
   absolute path), paths, fds, dirents, stat data and errnos onto `FS.*`
   calls; stderr/stdout are line-buffered and forwarded to the worker console
   and show up in the page log. See `alc-wasi.js`.

   Pitfalls the shim handles (each one broke a run before it was fixed):
   preview1 errnos and filetypes use their own numbering (e.g. `ENOENT` 44,
   `EINVAL` 28, directory 3, symlink 7); `__wasi_fdstat_t` puts rights at
   @8/@16 and wasi-libc masks every `openat` with the dirfd's inheriting
   rights; dirents start with `d_next`, have no length prefix, and a full
   buffer means "more entries"; `clock_time_get` writes one u64 of
   nanoseconds; this Emscripten build's `FS.analyzePath` has no `isDirectory`,
   `FS.cwd` is a function, and MEMFS nodes have no `length` (size comes from
   `getattr`). Add `?wasitrace=1` to the page URL to trace every WASI call,
   `?lognewest=1` to show the log newest-first.
6. **Format dump name** (pdfTeX only) — `compileformat` reads back
   `WORKROOT/pdflatex.fmt`, but the dump it just wrote is named after the
   kpathsea cache id of `pdflatex.ini`; the patch asks for the name the page
   seeds the dump back under. See “The format dump” below.

**TeX distribution files.** The SwiftLaTeX mirror (`texlive2.swiftlatex.com`)
is offline (Cloudflare 522) and sends no CORS headers anyway. Run the test
server `wasm-test/serve.py`: it proxies the engines' `<engine>/<fmt>/<file>`
requests through (a) an on-disk cache, (b) the **local TeX Live** resolved
with `kpsewhich` (font base names get `.tfm`/`.pfb`/… appended), (c) the
mirror as last resort.

**The format dump.** SwiftLaTeX stubs out pdfTeX's recorder, and its
`compileformat` routine reads back the wrong file name (the dump is named
after the kpathsea cache id of `pdflatex.ini` / `xelatex.ini`). The test page
builds the LaTeX format dump with the engine itself once, grabs the fresh
`*.fmt` from the worker's `/work`, seeds it as `swiftlatexpdftex.fmt` /
`swiftlatexxetex.fmt`, and caches it in IndexedDB (database
`alc-ng-wasm-test`, keys `alc_ng_swiftlatex{pdftex,xetex}_fmt_v1`) so later
runs skip the ~3 minute build. The XeLaTeX dump is ~27 MB. A built dump looked to the engine like any other seeded
file afterwards.

`wasm-test/texlive-cache/` is the proxy's fetch cache (gitignored).
