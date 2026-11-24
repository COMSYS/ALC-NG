import workerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url'

// pdf.js renders the compiled PDF in the page itself, so the PDF tab does not
// depend on the browser's own viewer (which some browsers lack, and which
// cannot show blob: PDFs inline on iOS). The library is a separate chunk,
// loaded the first time a PDF is shown.

type PdfJs = typeof import('pdfjs-dist')

let loading: Promise<PdfJs> | null = null

export function loadPdfJs(): Promise<PdfJs> {
  loading ??= import('pdfjs-dist').then((pdfjs) => {
    pdfjs.GlobalWorkerOptions.workerSrc = workerUrl
    return pdfjs
  })
  loading.catch(() => {
    loading = null
  })
  return loading
}

// The data pdf.js loads on demand, served next to the bundle (see
// vite.config.ts).
const base = import.meta.env.PDFJS_BASE

// Starts loading a document; the task's `promise` yields it, and `destroy()`
// releases it (in the worker as well).
export async function openPdf(bytes: Uint8Array) {
  const pdfjs = await loadPdfJs()
  return pdfjs.getDocument({
    data: bytes,
    cMapUrl: `${base}cmaps/`,
    standardFontDataUrl: `${base}standard_fonts/`,
    wasmUrl: `${base}wasm/`,
    iccUrl: `${base}iccs/`,
  })
}
