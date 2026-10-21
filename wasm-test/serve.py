#!/usr/bin/env python3
"""Serves the repository for wasm-test/index.html and backs the engine's
texlive-on-demand requests with the local TeX Live installation.

The real SwiftLaTeX mirror (https://texlive2.swiftlatex.com/) is offline and
sends no CORS headers anyway. The engine workers fetch every kpathsea file
from `<endpoint><engine>/<format>/<name>` (engine `pdftex` or `xetex`; the
dvipdfmx worker uses `xetex`); this server proxies GET `/texlive/<path>` as:

1. the per-URL cache under wasm-test/texlive-cache/ (gitignored),
2. the local TeX Live resolved with `kpsewhich <basename>` for that engine,
3. the SwiftLaTeX mirror (as a last resort).

Usage (from anywhere):  python3 wasm-test/serve.py [port]
Then open:              http://localhost:<port>/wasm-test/
Set SERVE_VERBOSE=1 to log every /texlive/ request with its status.

GET /tree/<dir> additionally lists a directory of the repository as JSON, which
is how the page fills a sample given as a folder (see SAMPLES in index.html)
without hardcoding every path of a submission.
"""

import hashlib
import json
import os
import shutil
import subprocess
import sys
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import unquote

MIRROR = "https://texlive2.swiftlatex.com/"
THIS_DIR = Path(__file__).resolve().parent
REPO_ROOT = THIS_DIR.parent
CACHE_DIR = THIS_DIR / "texlive-cache"
FILEID_DIR = CACHE_DIR / "fileids"
# kpathsea format ids (kpse_file_format_type) of requests that arrive
# without an extension, mapped to the extension to try first.
FORMAT_EXT = {
    "3": ".tfm",
    "6": ".bib",
    "7": ".bst",
    "32": ".pfb",
    "33": ".vf",
    "36": ".ttf",
    "44": ".enc",
    "47": ".otf",
}
# ... and to the format name kpsewhich -format= takes, as a last resort.
FORMAT_NAME = {
    "11": "map",
    "32": "type1 fonts",
    "36": "truetype fonts",
    "41": "misc fonts",
    "44": "enc files",
    "45": "cmap files",
    "46": "subfont definition files",
    "47": "opentype fonts",
}


class Handler(SimpleHTTPRequestHandler):
    def do_GET(self):
        if self.path.startswith("/texlive/"):
            return self.proxy_texlive()
        if self.path.startswith("/tree/"):
            return self.list_tree()
        super().do_GET()

    def list_tree(self):
        """GET /tree/<dir> -> {"dir": "<dir>", "files": ["a.tex", "figs/a.png", …]}

        Paths are relative to <dir> and sorted, so the page can hand a whole
        submission folder to the same fetch loop it uses for the small samples;
        a fixture dropped under wasm-test/fixtures/ then needs one line in
        SAMPLES. Dotfiles are skipped. Local dev server, but the listing stays
        inside the repository and never follows out of it."""
        rel = unquote(self.path[len("/tree/") :].split("?", 1)[0]).strip("/")
        root = REPO_ROOT.resolve()
        target = (root / rel).resolve()
        if not target.is_relative_to(root) or not target.is_dir():
            return self.send_error(404, "no such directory")
        files = sorted(
            str(p.relative_to(target))
            for p in target.rglob("*")
            if p.is_file()
            and not any(part.startswith(".") for part in p.relative_to(target).parts)
        )
        body = json.dumps({"dir": rel, "files": files}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def proxy_texlive(self):
        rel = self.path[len("/texlive/") :]
        # XeTeX writes the cache path of every OpenType/TrueType font it
        # loaded (`/tex/<fileid>`) into the XDV; dvipdfmx strips `/tex/` and
        # asks for `xetex/<format>/<fileid>[.otf|.ttf]`. Map the id back to
        # its request.
        known = FILEID_DIR / rel.rsplit("/", 1)[-1].split(".", 1)[0]
        if known.is_file():
            rel = known.read_text()
        cache_file = CACHE_DIR / rel.replace("/", "__")
        try:
            data = None
            if cache_file.is_file():
                data = cache_file.read_bytes()
            if data is None:
                data = self.local_texlive_file(rel)
                if data is not None:
                    cache_file.parent.mkdir(parents=True, exist_ok=True)
                    cache_file.write_bytes(data)
            if data is None:
                data = self.fetch_mirror(rel)
                if data is not None:
                    cache_file.parent.mkdir(parents=True, exist_ok=True)
                    cache_file.write_bytes(data)
            if data is None:
                # Reply 301, the mirror protocol's "file does not exist"
                # marker: the engine worker caches negatives only for 301,
                # so a plain 404 would be re-requested (and re-paid) every
                # time.
                self.send_response(301)
                self.end_headers()
                return
        except Exception as err:  # proxy trouble: report as upstream error
            self.send_error(502, str(err))
            return
        self.send_response(200)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Content-Length", str(len(data)))
        # The engine worker derives its /tex cache path from this header -
        # keep it stable per request so repeated fetches rewrite the same file.

        fileid = hashlib.sha1(rel.encode()).hexdigest()[:16]
        id_file = FILEID_DIR / fileid
        if not id_file.is_file():
            FILEID_DIR.mkdir(parents=True, exist_ok=True)
            id_file.write_text(rel)
        self.send_header("fileid", fileid)
        self.send_header("pkid", fileid)
        self.end_headers()
        self.wfile.write(data)

    @staticmethod
    def local_texlive_file(rel):
        """Resolves `<...>/<basename>` against the local TeX Live with
        kpsewhich; returns the file content or None.

        C-level kpathsea callers (fonts, virtual fonts) request base names
        without their format's extension, so several extensions are tried
        and the first hit wins."""
        parts = rel.split("/")
        basename = parts[-1]
        if not basename or basename.startswith("."):
            return None
        if shutil.which("kpsewhich") is None:
            return None
        # `<engine>/<kpathsea format id>/<name>`; the engine picks the
        # program-specific search paths (tex/xelatex/… for XeTeX).
        engine = parts[0] if len(parts) == 3 else "pdftex"
        progname = {"pdftex": "pdflatex", "xetex": "xelatex"}.get(engine, engine)
        candidates = [basename]
        if "." not in basename:
            preferred = FORMAT_EXT.get(parts[1]) if len(parts) == 3 else None
            exts = (".tfm", ".pfb", ".vf", ".otf", ".ttf", ".enc", ".map")
            if preferred:
                exts = (preferred,) + tuple(e for e in exts if e != preferred)
            candidates += [basename + ext for ext in exts]
        base_cmd = ["kpsewhich", f"-engine={engine}", f"-progname={progname}"]
        lookups = [base_cmd + [name] for name in candidates]
        # Formats whose search path kpsewhich does not guess from the
        # suffix (e.g. XeTeX's TECkit mappings, `*.tec` in "misc fonts").
        format_name = FORMAT_NAME.get(parts[1]) if len(parts) == 3 else None
        if format_name:
            lookups.append(base_cmd + [f"-format={format_name}", basename])
        for cmd in lookups:
            try:
                result = subprocess.run(cmd, capture_output=True, text=True, timeout=60)
            except subprocess.TimeoutExpired:
                continue
            lines = result.stdout.strip().splitlines()
            path = lines[0].strip() if lines and lines[0].strip() else None
            if path and path.startswith("/") and Path(path).is_file():
                return Path(path).read_bytes()
        return None

    @staticmethod
    def fetch_mirror(rel):
        """The SwiftLaTeX mirror has been offline (Cloudflare 522) since at
        least late 2026 and answers slowly, stalling the engine worker's
        synchronous downloads. Kept for documentation only - resurrect it if
        the mirror ever comes back."""
        return None

    def log_message(self, fmt, *args):
        # Quiet by default; SERVE_VERBOSE=1 logs the engines' TeX Live requests.
        if os.environ.get("SERVE_VERBOSE") and "/texlive/" in self.path:
            sys.stderr.write(f"{self.path} -> {fmt % args}\n")


def main():
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8000
    handler = partial(NoCacheHandler, directory=str(REPO_ROOT))
    # SERVE_HOST=0.0.0.0 exposes the server (e.g. inside a container).
    server = ThreadingHTTPServer(
        (os.environ.get("SERVE_HOST", "127.0.0.1"), port), handler
    )
    print(f"Serving {REPO_ROOT} on http://localhost:{port}")
    print("texlive proxy: cache -> local kpsewhich -> mirror")
    server.serve_forever()


# Test server: always revalidate, so edits to the page and the patched
# engine worker take effect after a normal reload.
class NoCacheHandler(Handler):
    def end_headers(self):
        self.send_header("Cache-Control", "no-cache")
        super().end_headers()


if __name__ == "__main__":
    main()
