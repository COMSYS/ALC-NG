import { logInfo, logOk, logv } from '../state/log'

// pdfium from the wasm build of https://github.com/bblanchon/pdfium-binaries
// (served under /wasm-test/pdfium/lib/, see wasm-test/pdfium/README.md). It is
// Emscripten output that is not an ES module, so it is loaded with a script
// tag and picks up its configuration from a pre-existing global `Module`.

const PDFIUM_DIR = '/wasm-test/pdfium/lib/'
const RENDER_TARGET = 3000  // RENDER_CONFIG in compare/pixel_perfect.rs

export interface PdfiumModule {
  HEAPU8: Uint8Array
  _malloc(size: number): number
  _free(ptr: number): void
  _FPDF_InitLibrary(): void
  _FPDF_LoadMemDocument(ptr: number, size: number, password: number): number
  _FPDF_GetLastError(): number
  _FPDF_GetPageCount(doc: number): number
  _FPDF_CloseDocument(doc: number): void
  _FPDF_LoadPage(doc: number, index: number): number
  _FPDF_ClosePage(page: number): void
  _FPDF_GetPageWidthF(page: number): number
  _FPDF_GetPageHeightF(page: number): number
  _FPDFBitmap_Create(width: number, height: number, alpha: number): number
  _FPDFBitmap_FillRect(bitmap: number, left: number, top: number, width: number, height: number, color: number): void
  _FPDF_RenderPageBitmap(bitmap: number, page: number, x: number, y: number, w: number, h: number, rotate: number, flags: number): void
  _FPDFBitmap_GetBuffer(bitmap: number): number
  _FPDFBitmap_GetStride(bitmap: number): number
  _FPDFBitmap_Destroy(bitmap: number): void
}

interface PdfiumConfig {
  locateFile(file: string): string
  print(line: string): void
  printErr(line: string): void
  onRuntimeInitialized(): void
}

declare global {
  interface Window {
    Module?: PdfiumConfig
  }
}

let pdfiumPromise: Promise<PdfiumModule> | null = null

export function loadPdfium(): Promise<PdfiumModule> {
  pdfiumPromise ??= new Promise<PdfiumModule>((resolve, reject) => {
    const t0 = performance.now()
    const config: PdfiumConfig = {
      locateFile: (file) => PDFIUM_DIR + file,
      print: (line) => logv(`[pdfium] ${line}`),
      printErr: (line) => logv(`[pdfium] ${line}`),
      onRuntimeInitialized() {
        // the glue extends the configuration object into the module itself
        const mod = config as unknown as PdfiumModule
        mod._FPDF_InitLibrary()
        logOk(`pdfium ready (${(performance.now() - t0).toFixed(0)} ms)`)
        resolve(mod)
      },
    }
    window.Module = config
    const script = document.createElement('script')
    script.src = `${PDFIUM_DIR}pdfium.js`
    script.onerror = () => reject(new Error('pdfium.js not found — see wasm-test/pdfium/README.md'))
    document.head.appendChild(script)
    logInfo('loading pdfium (wasm) …')
  })
  pdfiumPromise.catch(() => {
    pdfiumPromise = null
  })
  return pdfiumPromise
}

export interface PdfDoc {
  doc: number
  count: number
  close(): void
}

export function pdfiumOpen(M: PdfiumModule, bytes: Uint8Array): PdfDoc {
  const ptr = M._malloc(bytes.length)
  M.HEAPU8.set(bytes, ptr)
  const doc = M._FPDF_LoadMemDocument(ptr, bytes.length, 0)
  if (!doc) {
    const code = M._FPDF_GetLastError()
    M._free(ptr)
    throw new Error(`pdfium could not open the PDF (error ${code})`)
  }
  return {
    doc,
    count: M._FPDF_GetPageCount(doc),
    close() {
      M._FPDF_CloseDocument(doc)
      M._free(ptr)
    },
  }
}

// A rendered page: BGRA pixels (Uint32, little endian: B | G<<8 | R<<16 | A<<24).
export interface PageImage {
  width: number
  height: number
  px: Uint32Array
}

export function pdfiumRenderPage(M: PdfiumModule, pdf: PdfDoc, index: number): PageImage | null {
  const page = M._FPDF_LoadPage(pdf.doc, index)
  if (!page) return null
  try {
    let w = M._FPDF_GetPageWidthF(page)
    let h = M._FPDF_GetPageHeightF(page)
    const rotate = w > h ? 1 : 0  // rotate_if_landscape(Degrees90)
    if (rotate) [w, h] = [h, w]
    const scale = Math.min(RENDER_TARGET / w, RENDER_TARGET / h)
    const W = Math.max(1, Math.round(w * scale))
    const H = Math.max(1, Math.round(h * scale))
    const bitmap = M._FPDFBitmap_Create(W, H, 1)
    if (!bitmap) throw new Error(`pdfium: out of memory for a ${W}×${H} bitmap`)
    M._FPDFBitmap_FillRect(bitmap, 0, 0, W, H, 0xffffffff)
    M._FPDF_RenderPageBitmap(bitmap, page, 0, 0, W, H, rotate, 1 /* FPDF_ANNOT */)
    const buffer = M._FPDFBitmap_GetBuffer(bitmap)
    const stride = M._FPDFBitmap_GetStride(bitmap)
    const px = new Uint32Array(W * H)
    const bytes = new Uint8Array(px.buffer)
    const heap = M.HEAPU8  // re-read: the heap may have been replaced by growth
    for (let y = 0; y < H; y++) bytes.set(heap.subarray(buffer + y * stride, buffer + y * stride + W * 4), y * W * 4)
    M._FPDFBitmap_Destroy(bitmap)
    return { width: W, height: H, px }
  } finally {
    M._FPDF_ClosePage(page)
  }
}
