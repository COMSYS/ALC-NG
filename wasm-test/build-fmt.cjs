// Builds a SwiftLaTeX LaTeX format dump headlessly, outside a browser.
//
// The worker glue is Emscripten output with its Node code paths intact, so it
// runs under node once the browser bits it expects are shimmed:
//   - self/postMessage (worker scope),
//   - importScripts (used once, for alc-wasi.js),
//   - XMLHttpRequest, which the engine uses *synchronously* for kpathsea
//     lookups. Node has no sync HTTP, so the shim shells out to curl.
//
// usage: node build-fmt.cjs <glue dir> <glue js> <texlive endpoint> <out file>
"use strict";

const fs = require("fs");
const path = require("path");
const os = require("os");
const { execFileSync } = require("child_process");

const [glueArg, glueJs, endpoint, outFile] = process.argv.slice(2);
if (!glueArg || !glueJs || !endpoint || !outFile) {
  console.error("usage: node build-fmt.cjs <glue dir> <glue js> <texlive endpoint> <out file>");
  process.exit(2);
}
// absolute before anything chdirs: later lookups must not depend on the cwd
const glueDir = path.resolve(glueArg);
const outPath = path.resolve(outFile);
// The server is read-only and creates no directories, so make the target's.
fs.mkdirSync(path.dirname(outPath), { recursive: true });

// --- worker scope -----------------------------------------------------------
globalThis.self = globalThis;
let lastMessage = null;
self.postMessage = (msg) => {
  if (msg?.cmd === "workerlog") {
    process.stderr.write(`[engine] ${msg.text ?? ""}\n`);
    return;
  }
  lastMessage = msg;
};
globalThis.importScripts = (name) => {
  const code = fs.readFileSync(path.join(glueDir, name), "utf8");
  // eslint-disable-next-line no-eval
  eval(code);
};

// --- synchronous XMLHttpRequest over curl -----------------------------------
// The kpathsea hook in the glue blocks on xhr.send(); the dev server answers
// every request from kpsewhich, so this has to be correct, not fast.
class SyncXHR {
  constructor() {
    this.status = 0;
    this.response = null;
    this.timeout = 0;
    this.headers = {};
  }
  open(_method, url) {
    this.url = url;
  }
  setRequestHeader() {}
  overrideMimeType() {}
  getResponseHeader(name) {
    return this.headers[String(name).toLowerCase()] ?? null;
  }
  send() {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "fmtxhr-"));
    const head = path.join(dir, "head");
    const body = path.join(dir, "body");
    try {
      const status = execFileSync(
        "curl",
        ["-sS", "--max-time", "180", "-o", body, "-D", head, "-w", "%{http_code}", this.url],
        { encoding: "utf8", maxBuffer: 1 << 20 },
      ).trim();
      this.status = Number(status) || 0;
      this.headers = {};
      for (const line of fs.readFileSync(head, "utf8").split(/\r?\n/)) {
        const i = line.indexOf(":");
        if (i > 0) this.headers[line.slice(0, i).trim().toLowerCase()] = line.slice(i + 1).trim();
      }
      // slice: a pooled Buffer's .buffer is the whole 8 KiB pool, not this file
      const raw = fs.readFileSync(body);
      this.response = raw.buffer.slice(raw.byteOffset, raw.byteOffset + raw.byteLength);
    } catch (err) {
      process.stderr.write(`[xhr] ${this.url}: ${err.message}\n`);
      this.status = 0;
      this.response = null;
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  }
}
globalThis.XMLHttpRequest = SyncXHR;

// --- drive the engine -------------------------------------------------------
// ENVIRONMENT_IS_WORKER is true (importScripts exists), so the glue builds its
// wasm path as "./<name>": run it from its own directory. Node's global fetch
// would also be tried first for the wasm and fail on a relative URL; drop it so
// the glue takes its file-read path.
process.chdir(glueDir);
globalThis.fetch = undefined;
// absolute: a bare "wasm-test/..." would be looked up as a package name
require(path.join(glueDir, glueJs));

function waitFor(predicate, what, ms = 900000) {
  const end = Date.now() + ms;
  return new Promise((resolve, reject) => {
    const tick = () => {
      if (predicate()) return resolve(lastMessage);
      if (Date.now() > end) return reject(new Error(`timed out waiting for ${what}`));
      setTimeout(tick, 200);
    };
    tick();
  });
}

(async () => {
  // the module reports readiness with a spontaneous {result: "ok"}
  await waitFor(() => lastMessage?.result === "ok", "engine start");
  console.log(`${glueJs} ready`);

  self.onmessage({ data: { cmd: "settexliveurl", url: endpoint } });
  console.log(`texlive endpoint ${endpoint}`);
  console.log("building the format (INITEX + latex.ltx, this takes a while)");
  const t0 = Date.now();
  lastMessage = null;
  self.onmessage({ data: { cmd: "compileformat" } });
  const reply = await waitFor(() => lastMessage && lastMessage.cmd === "compile", "compileformat");
  console.log(`format build finished in ${((Date.now() - t0) / 1000).toFixed(0)} s: ${reply.result} status ${reply.status}`);

  if (reply.result !== "ok" || !reply.pdf) {
    // The readback bug: the dump is named after the kpathsea cache id of the
    // engine's .ini, so read whatever *.fmt the build left in /work.
    const listing = await call("listdir");
    const name = (listing.names ?? []).find((n) => n.toLowerCase().endsWith(".fmt"));
    if (!name) throw new Error(`no *.fmt in /work (status ${reply.status})\n${reply.log ?? ""}`);
    console.log(`readback missed, taking ${name} from /work`);
    const file = await call("readfile", { url: name });
    if (file.content === undefined) throw new Error(`could not read ${name}`);
    fs.writeFileSync(outPath, Buffer.from(file.content));
  } else {
    fs.writeFileSync(outPath, Buffer.from(reply.pdf));
  }
  console.log(`wrote ${outPath} (${(fs.statSync(outPath).size / (1024 * 1024)).toFixed(1)} MB)`);
  process.exit(0);
})().catch((err) => {
  console.error(String(err.message ?? err));
  process.exit(1);
});

// one command round-trip against the engine
function call(cmd, extra = {}, ms = 60000) {
  lastMessage = null;
  self.onmessage({ data: { cmd, ...extra } });
  return waitFor(() => lastMessage && (lastMessage.cmd === cmd || lastMessage.result), cmd, ms);
}
