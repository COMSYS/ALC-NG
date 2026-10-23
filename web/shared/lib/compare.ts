import { compareStore, resetCompare, type CompareCard } from '../state/compare'
import { logErr, logInfo, logOk, logv } from '../state/log'
import type { CompareOutcome, CompileRun, PipelineConfig } from '../state/review'
import { steps } from '../state/steps'
import { compileCleanedInEngine } from './compile'
import { loadPdfium, pdfiumOpen, pdfiumRenderPage, type PageImage, type PdfDoc } from './pdfium'

// --- compare flag (pixel-perfect PDF comparison) --------------------------------
//
// Native alc-ng compiles the cleaned folder, renders the original and the new
// PDF with pdfium (3000 px wide, at most 3000 px high, landscape pages rotated
// by 90°) and writes an image diff for every page that is not pixel-identical.
// Here the original PDF is the one the engine compiled before cleaning; the
// cleaned folder is compiled in the same engine. The Rust Comparer is
// native-only (it binds the pdfium dynamic library), so this part mirrors
// compare/pixel_perfect.rs and helper::image_diff.

export interface RgbaImage {
  width: number
  height: number
  rgba: Uint32Array
}

const bgraToRgba = (v: number) => ((v & 0xff00ff00) | ((v & 0xff) << 16) | ((v >>> 16) & 0xff)) >>> 0

export function pagesEqual(a: PageImage, b: PageImage): boolean {
  if (a.width !== b.width || a.height !== b.height) return false
  for (let i = 0; i < a.px.length; i++) if (a.px[i] !== b.px[i]) return false
  return true
}

// helper::image_diff: pixels outside either image and pixels whose RGB differs
// take the change color; all others come from the cleaned ("after") page.
export function imageDiff(before: PageImage, after: PageImage, [r, g, b]: [number, number, number]): RgbaImage {
  const W = Math.max(before.width, after.width)
  const H = Math.max(before.height, after.height)
  const out = new Uint32Array(W * H)
  const changed = (r | (g << 8) | (b << 16) | (255 << 24)) >>> 0
  const minW = Math.min(before.width, after.width)
  const minH = Math.min(before.height, after.height)
  for (let y = 0; y < H; y++) {
    for (let x = 0; x < W; x++) {
      let v = changed
      if (x < minW && y < minH) {
        const p = before.px[y * before.width + x]
        const q = after.px[y * after.width + x]
        if (!((p ^ q) & 0x00ffffff)) v = bgraToRgba((q & 0x00ffffff) | (p & 0xff000000))
      }
      out[y * W + x] = v
    }
  }
  return { width: W, height: H, rgba: out }
}

function pageToRgba(page: PageImage): RgbaImage {
  const out = new Uint32Array(page.px.length)
  for (let i = 0; i < out.length; i++) out[i] = bgraToRgba(page.px[i])
  return { width: page.width, height: page.height, rgba: out }
}

function rgbaToJpegBlob({ width, height, rgba }: RgbaImage): Promise<Blob> {
  const canvas = document.createElement('canvas')
  canvas.width = width
  canvas.height = height
  canvas.getContext('2d')!.putImageData(new ImageData(new Uint8ClampedArray(rgba.buffer as ArrayBuffer), width, height), 0, 0)
  return new Promise((resolve, reject) =>
    canvas.toBlob((blob) => (blob ? resolve(blob) : reject(new Error('JPEG encoding failed'))), 'image/jpeg', 0.92),
  )
}

export function parseHexColor(hex: string): [number, number, number] {
  const m = /^#?([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(hex.trim())
  if (!m) throw new Error(`Invalid hex color: '${hex}'`)
  return [parseInt(m[1], 16), parseInt(m[2], 16), parseInt(m[3], 16)]
}

// Returns a one-line outcome for the status line; never throws. The
// comparison runs behind the progress sheet: the workspace stays on the
// review tab so the compare step and its live page-by-page detail remain
// visible; the results wait on their tab.
export async function runCompare(cfg: PipelineConfig, run: CompileRun, diffColor: string): Promise<CompareOutcome> {
  resetCompare()
  compareStore.set({ summary: { state: 'running' } })
  const t0 = performance.now()
  let originalDoc: PdfDoc | null = null
  let cleanedDoc: PdfDoc | null = null
  try {
    const color = parseHexColor(diffColor)

    steps.detail('compare', 'Compiling the cleaned sources')
    const cleanedPdf = await compileCleanedInEngine(cfg, run.main)
    if (!cleanedPdf) {
      logErr('compare: the cleaned main file did not compile')
      compareStore.set({ summary: { state: 'failed', message: `The cleaned version of ${run.main} failed to compile — see the processing log.` } })
      return { state: 'failed', text: 'the cleaned version did not compile' }
    }

    steps.detail('compare', 'Rendering both PDFs')
    const M = await loadPdfium()
    originalDoc = pdfiumOpen(M, run.pdf)
    cleanedDoc = pdfiumOpen(M, cleanedPdf)
    logInfo(`compare ${run.main}: ${originalDoc.count} original page(s), ${cleanedDoc.count} cleaned page(s)`)

    const cards: CompareCard[] = []
    const pageCount = Math.max(originalDoc.count, cleanedDoc.count)
    for (let i = 0; i < pageCount; i++) {
      steps.detail('compare', `Page ${i + 1} of ${pageCount}`)
      await new Promise((r) => setTimeout(r, 0))  // let the status paint
      const left = i < originalDoc.count ? pdfiumRenderPage(M, originalDoc, i) : null
      const right = i < cleanedDoc.count ? pdfiumRenderPage(M, cleanedDoc, i) : null
      let image: RgbaImage
      let kind: CompareCard['kind']
      if (left && right) {
        if (pagesEqual(left, right)) {
          logv(`compare: page ${i} identical`)
          continue
        }
        image = imageDiff(left, right, color)
        kind = 'diff'
      } else if (left) {
        image = pageToRgba(left)
        kind = 'original only'
      } else if (right) {
        image = pageToRgba(right)
        kind = 'cleaned only'
      } else {
        continue
      }
      const blob = await rgbaToJpegBlob(image)
      cards.push({
        index: i,
        kind,
        url: URL.createObjectURL(blob),
        name: `${run.main}_${i}.jpg`.replace(/\//g, '_'),
        size: blob.size,
      })
      logInfo(`compare: page ${i} differs (${kind})`)
    }

    const ms = Number((performance.now() - t0).toFixed(0))
    if (cards.length === 0) {
      logOk(`compare: pixel-identical (${pageCount} page(s), ${ms} ms)`)
      compareStore.set({ summary: { state: 'ok', main: run.main, pages: pageCount, ms } })
      return { state: 'ok', text: 'pixel-identical', pages: pageCount, ms }
    }
    logErr(`compare: ${run.main} has differing pages: ${cards.map((c) => c.index).join(', ')} (${ms} ms)`)
    compareStore.set({ summary: { state: 'differs', main: run.main, differing: cards.length, pages: pageCount, ms }, cards })
    return { state: 'differs', text: `${cards.length} differing page(s)`, differing: cards.length, pages: pageCount, ms }
  } catch (err) {
    const message = String((err as Error)?.message ?? err)
    logErr(`compare failed: ${message}`)
    compareStore.set({ summary: { state: 'failed', message: `Compare failed: ${message}` } })
    return { state: 'failed', text: 'the comparison failed' }
  } finally {
    originalDoc?.close()
    cleanedDoc?.close()
  }
}
