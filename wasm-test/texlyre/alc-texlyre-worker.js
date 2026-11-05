/* alc-ng adapter: drives the TeXlyre BusyTeX pipeline (TeX Live 2026, one
 * wasm module for pdfTeX / XeTeX / xdvipdfmx / bibtex8 / makeindex) through
 * the worker protocol of the patched SwiftLaTeX engines, so the page
 * (web/shared/lib/engine.ts) drives both backends the same way.
 *
 * Spawn as `alc-texlyre-worker.js?engine=pdftex|xetex`. The engine picks the
 * BusyTeX driver (`pdftex_bibtex8`, `xetex_bibtex8_dvipdfmx`) and the TeX Live
 * proxy route (`/texlive/<engine>/<format>/<name>`, the contract the dev
 * server already speaks to the SwiftLaTeX workers).
 *
 * Commands handled (replies as in the SwiftLaTeX workers):
 *   settexliveurl {url}        TeX Live proxy base
 *   writefile {url, src}       stage a file under /work           -> ok
 *   mkdir {url}                stage an (empty) directory         -> ok
 *   setmainfile {url, dir}     main file and its directory        (no reply)
 *   compilelatex               the whole BusyTeX run: TeX passes, BibTeX or
 *                              biber, makeindex, xdvipdfmx for XeTeX
 *                              -> {result, status, log, pdf, cmd: "compile"}
 *   flushcache                 forget the staged files            (no reply)
 *   seedfiles {files}          no-op: the formats ship prebuilt   -> ok
 *   listdir / readfile {url}   inspect /work
 *   openlog                    drain the file access log          -> {entries}
 *   alcng_*                    the WASI shim (../swiftlatex/alc-wasi.js) on
 *                              the pipeline's Emscripten FS
 *
 * What differs from the SwiftLaTeX workers (the page knows via its engine
 * spec, web/shared/lib/engine.ts): one compilelatex runs every pass itself;
 * XeTeX returns a PDF, not an XDV. The access log is recorded the way the
 * patched SwiftLaTeX workers do it, at the open calls of the engines (here
 * FS.open, while an applet runs), which also covers bibtex8.
 */
"use strict";

importScripts("busytex_biber.js", "busytex_pipeline.js", "../swiftlatex/alc-wasi.js");

const ENGINE = new URL(self.location.href).searchParams.get("engine") === "xetex" ? "xetex" : "pdftex";
const DRIVER = ENGINE === "xetex" ? "xetex_bibtex8_dvipdfmx" : "pdftex_bibtex8";
const WORK = "/work";  // the pipeline's project directory, like the SwiftLaTeX workers'

const here = (name) => new URL(name, self.location.href).href;
const post = (message, transfer) => self.postMessage(message, transfer || []);
const workerlog = (level, text) => post({ cmd: "workerlog", level, text });

// The module's kpathsea hook fetches TeX Live files with synchronous XHRs.
// The page counts the log line as a TeX Live download (forwardWorkerLog);
// `?miss=404` makes the dev server answer a missing file with a 404, the
// status the module remembers as a negative (it answers 301 otherwise, the
// SwiftLaTeX workers' marker).
const xhrOpen = XMLHttpRequest.prototype.open;
XMLHttpRequest.prototype.open = function (method, url, ...rest) {
  if (String(url).includes("/texlive/")) {
    url = `${url}?miss=404`;
    workerlog("log", `Start downloading texlive file ${url}`);
  }
  return xhrOpen.call(this, method, url, ...rest);
};

let pipeline = null;
let texliveBase = `${self.location.origin}/texlive/`;

// The staged submission (what the page wrote with writefile/mkdir) and the
// main file. BusyTeX mounts a fresh MEMFS at /work on every compile and
// writes `staged` into it; `live` says /work currently holds a compile.
const staged = new Map();   // rel path -> Uint8Array
const stagedDirs = new Set();
let mainFile = { name: "", dir: "" };
let live = false;
let accessLog = [];  // {p, m} per distinct open since the last openlog drain
const accessSeen = new Set();
let recording = false;

const texliveEndpoint = () => `${texliveBase}${texliveBase.endsWith("/") ? "" : "/"}${ENGINE}`;
const mainRel = () => (mainFile.dir ? `${mainFile.dir}/${mainFile.name}` : mainFile.name);
const stemRel = () => mainRel().replace(/\.[^./]*$/, "");

async function exists(url) {
  try {
    const response = await fetch(url, { method: "HEAD" });
    return response.ok && !/text\/html/.test(response.headers.get("content-type") || "");
  } catch (e) {
    return false;
  }
}

async function init() {
  const basic = here("texlive-basic.js");
  if (!(await exists(basic)) || !(await exists(here("texlive-basic.data")))) {
    throw new Error("texlive-basic.{js,data} not found next to the worker — run wasm-test/fetch-texlyre.sh");
  }
  const biber = (await exists(here("biber.js"))) && (await exists(here("biber.wasm"))) && (await exists(here("biber.data")));

  // Only `basic` goes to the pipeline: the rest of TeX Live comes from the
  // proxy on demand, and the package resolver would otherwise download the
  // multi-hundred-MB recommended/extra sets for any \usepackage it can match.
  pipeline = new BusytexPipeline(
    here("busytex.js"), here("busytex.wasm"), [basic], [basic], [],
    (text) => workerlog("log", text),
    () => {},
    false,  // no module yet: the project dir below must be set first
    BusytexPipeline.ScriptLoaderWorker,
    biber ? here("biber.js") : null, biber ? here("biber.wasm") : null, biber ? here("biber.data") : null,
  );

  // The module is created with the pipeline's project dir, and with
  // `preload` off it would be thrown away after every compile.
  pipeline.preload = true;
  pipeline.project_dir = WORK;

  // Every module the pipeline creates gets the access log hook, and the
  // log only records while an applet runs: the pipeline stages the
  // submission with FS.writeFile, which must not count as engine output.
  const reload = pipeline.reload_module.bind(pipeline);
  pipeline.reload_module = async (...args) => {
    const module = await reload(...args);
    hookOpen(module.FS);
    return module;
  };
  const runCmd = pipeline._run_cmd.bind(pipeline);
  pipeline._run_cmd = (...args) => {
    recording = true;
    try {
      return runCmd(...args);
    } finally {
      recording = false;
    }
  };

  pipeline.Module = pipeline.reload_module(pipeline.env, WORK, [basic], true);
  await pipeline.Module;
}

// --- file helpers -----------------------------------------------------------

function mkdirs(FS, dir) {
  let path = "";
  for (const part of dir.split("/")) {
    if (!part) continue;
    path += `/${part}`;
    if (!FS.analyzePath(path).exists) FS.mkdir(path);
  }
}

function writeInto(FS, rel, bytes) {
  const full = `${WORK}/${rel}`;
  mkdirs(FS, full.slice(0, full.lastIndexOf("/")));
  FS.writeFile(full, bytes);
}

const readText = (FS, path) => (FS.analyzePath(path).exists ? FS.readFile(path, { encoding: "utf8" }) : "");

// Copies a tree between two Emscripten filesystems (the pipeline replaces its
// module when a compile needs more data packages; the cleaned tree must
// survive that).
function copyTree(from, to, path) {
  const info = from.analyzePath(path);
  if (!info.exists) return;
  if (!from.isDir(info.object.mode)) {
    mkdirs(to, path.slice(0, path.lastIndexOf("/")) || "/");
    to.writeFile(path, from.readFile(path));
    return;
  }
  mkdirs(to, path);
  for (const name of from.readdir(path)) {
    if (name !== "." && name !== "..") copyTree(from, to, `${path}/${name}`);
  }
}

// --- the access log ---------------------------------------------------------

// Entries {p: path as opened, m: 1 for write-only, else 0}, the shape the
// SwiftLaTeX workers' openlog returns.
function hookOpen(FS) {
  const open = FS.open;
  FS.open = function (path, flags, ...rest) {
    if (recording && typeof path === "string") {
      const writeOnly = typeof flags === "string" ? /^[wa]/.test(flags) && !flags.includes("+") : (flags & 3) === 1;
      const m = writeOnly ? 1 : 0;
      const key = `${m} ${path}`;
      if (!accessSeen.has(key)) {
        accessSeen.add(key);
        accessLog.push({ p: path, m });
        // synctex writes <job>.synctex(busy) and renames it to .synctex.gz
        if (path.endsWith(".synctex(busy)")) accessLog.push({ p: path.replace("(busy)", ".gz"), m: 1 });
      }
    }
    return open.call(this, path, flags, ...rest);
  };
}

// --- compile ----------------------------------------------------------------

async function compile() {
  if (!mainFile.name) return post({ result: "failed", cmd: "compile", status: -1, log: "no main file set" });
  const files = [...stagedDirs].map((path) => ({ path, contents: null }));
  for (const [path, contents] of staged) files.push({ path, contents });

  const before = await pipeline.Module;
  // Contents go in as bytes on purpose: the pipeline's package resolver only
  // reads strings, so no data package beyond `basic` is ever downloaded.
  // BibTeX is left to the pipeline's detection (\bibliography in a .tex) only
  // when a .bib is staged: without one (a cleaned tree keeps just the .bbl)
  // it would fail yet still replace the .bbl with an empty file.
  const bibtex = [...staged.keys()].some((path) => /\.bib$/i.test(path)) ? null : false;
  const result = await pipeline.compile(
    files, mainRel(), bibtex, null, null, true, BusytexPipeline.VerboseSilent, DRIVER, [], texliveEndpoint(), false,
  );
  live = true;
  const module = await pipeline.Module;
  const FS = module.FS;
  if (module !== before) {
    for (const path of ["/cleaned", "/cleaned.tar.gz"]) copyTree(before.FS, FS, path);
  }

  // The TeX log of the last pass; the pipeline's own concatenation (all
  // commands, stdout and stderr) when TeX wrote none.
  const texLog = readText(FS, `${WORK}/${stemRel()}.log`);
  const log = texLog ? `${texLog}\n${result.logs.map((entry) => entry.stdout).filter(Boolean).slice(-1)}` : result.log;
  if (result.exit_code === 0 && result.pdf && result.pdf.length > 0) {
    post({ result: "ok", cmd: "compile", status: 0, log, pdf: result.pdf.buffer }, [result.pdf.buffer]);
  } else {
    post({ result: "failed", cmd: "compile", status: result.exit_code, log });
  }
}

// --- dispatch ---------------------------------------------------------------

let tail = Promise.resolve();

async function handle(data) {
  const cmd = data && data.cmd;
  const FS = pipeline && (await pipeline.Module) ? (await pipeline.Module).FS : null;
  switch (cmd) {
    case "settexliveurl":
      texliveBase = new URL(data.url, self.location.href).href;
      break;
    case "writefile":
      staged.set(data.url, data.src);
      if (live && FS) writeInto(FS, data.url, data.src);
      post({ result: "ok", cmd });
      break;
    case "mkdir":
      stagedDirs.add(data.url);
      post({ result: "ok", cmd });
      break;
    case "setmainfile":
      mainFile = { name: data.url, dir: data.dir || "" };
      break;
    case "flushcache":
      staged.clear();
      stagedDirs.clear();
      accessLog = [];
      accessSeen.clear();
      live = false;
      break;
    case "seedfiles":
      post({ result: "ok", cmd });
      break;
    case "compilelatex":
      try {
        await compile();
      } catch (err) {
        workerlog("error", `compile failed: ${(err && err.stack) || err}`);
        post({ result: "failed", cmd: "compile", status: -1, log: String((err && err.message) || err) });
      }
      break;
    case "openlog": {
      const entries = accessLog;
      accessLog = [];
      accessSeen.clear();
      post({ result: "ok", cmd, entries });
      break;
    }
    case "listdir":
      try {
        post({ result: "ok", cmd, names: FS.readdir(WORK).filter((n) => n !== "." && n !== "..") });
      } catch (err) {
        post({ result: "failed", cmd, error: String(err) });
      }
      break;
    case "readfile":
      try {
        const bytes = FS.readFile(`${WORK}/${data.url}`);
        post({ result: "ok", cmd, content: bytes.buffer }, [bytes.buffer]);
      } catch (err) {
        post({ result: "failed", cmd, error: String(err) });
      }
      break;
    default:
      if (!FS || !AlcWasi.dispatch(data, FS)) workerlog("warn", `unknown command ${cmd}`);
  }
}

self.onmessage = (ev) => {
  tail = tail.then(() => handle(ev.data)).catch((err) => workerlog("error", `${ev.data && ev.data.cmd}: ${(err && err.stack) || err}`));
};

init().then(
  () => post({ result: "ok" }),  // the readiness signal the page waits for
  // An uncaught error reaches the page's worker.onerror, which drops the engine.
  (err) => setTimeout(() => { throw new Error(`BusyTeX failed to initialize: ${(err && err.message) || err}`); }),
);
