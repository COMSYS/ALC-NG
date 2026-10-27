import react from '@vitejs/plugin-react'
import { createReadStream, readdirSync, readFileSync, statSync } from 'node:fs'
import { createRequire } from 'node:module'
import { dirname, join, normalize } from 'node:path'
import type { Plugin } from 'vite'
import { defineConfig } from 'vitest/config'

// Everything the page loads besides its own bundle comes from the axum dev
// server (wasm-test/server): the TeX Live proxy, the prebuilt format dumps,
// the /tree/ listings, the patched SwiftLaTeX workers and pdfium under
// /wasm-test/, the alc_ng.wasm build under /target/, and the samples under
// /wasm-test/fixtures/ and /tests/. `pnpm dev` forwards those paths to it;
// ALC_SERVER points elsewhere than the server's default port.
const server = process.env.ALC_SERVER ?? 'http://127.0.0.1:8000'
const proxied = ['/texlive', '/fmt', '/tree', '/wasm-test', '/target', '/tests']

// pdf.js loads some data on demand from plain URLs: CMaps for CJK text,
// metrics of the 14 standard fonts when a PDF does not embed them, the
// WebAssembly image decoders (JPEG 2000, JBIG2) and ICC profiles. They are
// served below /assets/ under the pdf.js version, so the server's permanent
// caching of /assets/ stays correct across upgrades (see src/lib/pdfjs.ts).
const require = createRequire(import.meta.url)
const pdfjsRoot = dirname(require.resolve('pdfjs-dist/package.json'))
const pdfjsVersion = (JSON.parse(readFileSync(join(pdfjsRoot, 'package.json'), 'utf8')) as { version: string }).version
const PDFJS_DATA_DIRS = ['cmaps', 'standard_fonts', 'wasm', 'iccs']
const PDFJS_BASE = `/assets/pdfjs-${pdfjsVersion}/`

function pdfjsData(): Plugin {
  return {
    name: 'alc-ng:pdfjs-data',
    config: () => ({ define: { 'import.meta.env.PDFJS_BASE': JSON.stringify(PDFJS_BASE) } }),
    configureServer(dev) {
      dev.middlewares.use(PDFJS_BASE, (req, res, next) => {
        const rel = normalize(decodeURIComponent((req.url ?? '').split('?')[0])).replace(/^[/\\]+/, '')
        const [dir] = rel.split(/[/\\]/)
        const file = join(pdfjsRoot, rel)
        if (!PDFJS_DATA_DIRS.includes(dir) || !file.startsWith(pdfjsRoot) || !statSync(file, { throwIfNoEntry: false })?.isFile()) {
          next()
          return
        }
        if (file.endsWith('.wasm')) res.setHeader('Content-Type', 'application/wasm')
        createReadStream(file).pipe(res)
      })
    },
    generateBundle() {
      for (const dir of PDFJS_DATA_DIRS) {
        for (const name of readdirSync(join(pdfjsRoot, dir))) {
          this.emitFile({
            type: 'asset',
            fileName: `${PDFJS_BASE.slice(1)}${dir}/${name}`,
            source: readFileSync(join(pdfjsRoot, dir, name)),
          })
        }
      }
    },
  }
}

export default defineConfig({
  plugins: [react(), pdfjsData()],
  server: {
    proxy: Object.fromEntries(proxied.map((path) => [path, { target: server }])),
  },
  build: {
    // The server serves the build from web/dist: the overview index.html
    // at the site root, one page per design under /tool/ and /site/, and
    // the hashed bundles of both under /assets/ (see wasm-test/server).
    // The code under shared/ ends up in chunks both pages load.
    outDir: 'dist',
    assetsDir: 'assets',
    rollupOptions: {
      input: {
        index: 'index.html',
        tool: 'tool/index.html',
        site: 'site/index.html',
      },
    },
  },
  test: {
    environment: 'node',
  },
})
