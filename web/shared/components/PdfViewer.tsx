import type { PDFDocumentLoadingTask, PDFDocumentProxy, RenderTask, TextLayer } from 'pdfjs-dist'
import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from 'react'
import { loadPdfJs, openPdf } from '../lib/pdfjs'
import { logErr } from '../state/log'

// A PDF point in CSS pixels: what PDF viewers call 100 %.
const CSS_UNITS = 96 / 72
const ZOOM_STEPS = [0.5, 0.67, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3, 4]
// space around the pages inside the scroll area
const GUTTER = 16

type Zoom = 'fit' | number

interface PageSize {
  width: number
  height: number
}

const isCancelled = (err: unknown) => (err as { name?: string })?.name === 'RenderingCancelledException'

// The compiled PDF, rendered with pdf.js: every page as a canvas with a text
// layer on top (so text can be selected, copied and found with the browser's
// search), drawn only while it is near the visible part of the document.
// Mount it with a `key` per document, so a new PDF starts from fresh state.
export function PdfViewer({ bytes, url }: { bytes: Uint8Array; url: string }) {
  const [doc, setDoc] = useState<PDFDocumentProxy | null>(null)
  const [sizes, setSizes] = useState<PageSize[]>([])
  const [error, setError] = useState<string | null>(null)
  const [zoom, setZoom] = useState<Zoom>('fit')
  const [width, setWidth] = useState(0)
  // the scroll area: state to render the pages once it exists, a ref to
  // read and move its scroll position
  const [scroller, setScroller] = useState<HTMLDivElement | null>(null)
  const scrollRef = useRef<HTMLDivElement | null>(null)
  const [current, setCurrent] = useState(1)
  const pageEls = useRef<(HTMLDivElement | null)[]>([])

  // load the document and the size of every page
  useEffect(() => {
    let cancelled = false
    let task: PDFDocumentLoadingTask | null = null
    // pdf.js may hand the buffer to its worker, which would empty it here
    openPdf(bytes.slice())
      .then((loading) => {
        task = loading
        return loading.promise
      })
      .then(async (pdf) => {
        const all: PageSize[] = []
        for (let i = 1; i <= pdf.numPages && !cancelled; i++) {
          const viewport = (await pdf.getPage(i)).getViewport({ scale: CSS_UNITS })
          all.push({ width: viewport.width, height: viewport.height })
        }
        if (cancelled) return
        setSizes(all)
        setDoc(pdf)
      })
      .catch((err: Error) => {
        if (cancelled) return
        setError(err.message)
        logErr(`PDF viewer: ${err.message}`)
      })
    return () => {
      cancelled = true
      void (task as PDFDocumentLoadingTask | null)?.destroy()
    }
  }, [bytes])

  // the width "fit" fills; a hidden tab reports 0 and keeps the last value
  useEffect(() => {
    if (!scroller) return
    const observer = new ResizeObserver(([entry]) => {
      if (entry.contentRect.width > 0) setWidth(entry.contentRect.width)
    })
    observer.observe(scroller)
    return () => observer.disconnect()
  }, [scroller])

  const widest = Math.max(1, ...sizes.map((s) => s.width))
  const fitScale = width > 0 ? Math.max(0.25, (width - 2 * GUTTER) / widest) : 1
  const scale = zoom === 'fit' ? fitScale : zoom

  // keep the reader on the same spot of the document when the scale changes
  const lastScale = useRef(scale)
  useLayoutEffect(() => {
    const el = scrollRef.current
    if (el && lastScale.current !== scale) el.scrollTop *= scale / lastScale.current
    lastScale.current = scale
  }, [scale])

  const onScroll = () => {
    const el = scrollRef.current
    if (!el) return
    const mark = el.scrollTop + el.clientHeight / 3
    let page = 1
    pageEls.current.forEach((el, i) => {
      if (el && el.offsetTop <= mark) page = i + 1
    })
    setCurrent(page)
  }

  const goTo = (page: number) => pageEls.current[page - 1]?.scrollIntoView({ block: 'start' })
  const zoomBy = (direction: 1 | -1) => {
    const next =
      direction > 0 ? ZOOM_STEPS.find((z) => z > scale + 0.001) : [...ZOOM_STEPS].reverse().find((z) => z < scale - 0.001)
    setZoom(next ?? scale)
  }
  const pages = sizes.length

  return (
    <div className="pdfViewer">
      <div className="pdfToolbar" role="toolbar" aria-label="PDF">
        <button type="button" className="button small" disabled={current <= 1} onClick={() => goTo(current - 1)} aria-label="Previous page">
          ‹
        </button>
        <span className="pdfPageNo">{pages > 0 ? `Page ${current} of ${pages}` : 'Loading…'}</span>
        <button type="button" className="button small" disabled={current >= pages} onClick={() => goTo(current + 1)} aria-label="Next page">
          ›
        </button>
        <span className="spacer" />
        <button type="button" className="button small" disabled={scale <= ZOOM_STEPS[0]} onClick={() => zoomBy(-1)} aria-label="Zoom out">
          −
        </button>
        <span className="pdfZoom">{Math.round(scale * 100)}%</span>
        <button
          type="button"
          className="button small"
          disabled={scale >= ZOOM_STEPS[ZOOM_STEPS.length - 1]}
          onClick={() => zoomBy(1)}
          aria-label="Zoom in"
        >
          +
        </button>
        <button type="button" className="button small" aria-pressed={zoom === 'fit'} onClick={() => setZoom('fit')}>
          Fit width
        </button>
        <span className="spacer" />
        <a className="button small" href={url} target="_blank" rel="noopener">
          Open
        </a>
        <a className="button small" href={url} download="compiled.pdf">
          Save
        </a>
      </div>
      <div
        className="pdfScroll"
        ref={(el) => {
          scrollRef.current = el
          setScroller(el)
        }}
        onScroll={onScroll}
      >
        {error !== null && <p className="diffNote">The PDF could not be shown: {error}</p>}
        {doc &&
          scroller &&
          sizes.map((size, i) => (
            <PdfPage
              key={i}
              doc={doc}
              number={i + 1}
              size={size}
              scale={scale}
              root={scroller}
              ref={(el) => {
                pageEls.current[i] = el
              }}
            />
          ))}
      </div>
    </div>
  )
}

interface PageProps {
  doc: PDFDocumentProxy
  number: number
  size: PageSize
  scale: number
  root: HTMLElement
  ref: (el: HTMLDivElement | null) => void
}

function PdfPage({ doc, number, size, scale, root, ref }: PageProps) {
  const page = useRef<HTMLDivElement | null>(null)
  const canvasHost = useRef<HTMLDivElement>(null)
  const textHost = useRef<HTMLDivElement>(null)
  const [near, setNear] = useState(false)

  // draw only pages within about a screen of the visible part
  useEffect(() => {
    const el = page.current
    if (!el) return
    const observer = new IntersectionObserver(([entry]) => setNear(entry.isIntersecting), { root, rootMargin: '100% 0px' })
    observer.observe(el)
    return () => observer.disconnect()
  }, [root])

  useEffect(() => {
    const canvasEl = canvasHost.current
    const textEl = textHost.current
    if (!canvasEl || !textEl) return
    if (!near) {
      // release the bitmaps of pages far out of view
      canvasEl.replaceChildren()
      textEl.replaceChildren()
      return
    }
    let cancelled = false
    let task: RenderTask | null = null
    let text: TextLayer | null = null
    ;(async () => {
      const pdfjs = await loadPdfJs()
      const pdfPage = await doc.getPage(number)
      if (cancelled) return
      const viewport = pdfPage.getViewport({ scale: scale * CSS_UNITS })
      const ratio = window.devicePixelRatio || 1
      // render off-screen and swap, so a zoom never flashes a blank page
      const canvas = document.createElement('canvas')
      canvas.width = Math.floor(viewport.width * ratio)
      canvas.height = Math.floor(viewport.height * ratio)
      task = pdfPage.render({ canvas, viewport, transform: ratio !== 1 ? [ratio, 0, 0, ratio, 0, 0] : undefined })
      await task.promise
      if (cancelled) return
      canvasEl.replaceChildren(canvas)
      textEl.replaceChildren()
      text = new pdfjs.TextLayer({ textContentSource: pdfPage.streamTextContent(), container: textEl, viewport })
      await text.render()
    })().catch((err: unknown) => {
      if (!cancelled && !isCancelled(err)) logErr(`PDF viewer: page ${number}: ${(err as Error)?.message ?? err}`)
    })
    return () => {
      cancelled = true
      task?.cancel()
      text?.cancel()
    }
  }, [doc, number, scale, near])

  const style = {
    width: Math.floor(size.width * scale),
    height: Math.floor(size.height * scale),
    '--total-scale-factor': scale * CSS_UNITS,
    '--scale-round-x': '1px',
    '--scale-round-y': '1px',
  } as CSSProperties
  return (
    <div
      className="pdfPage"
      style={style}
      data-page={number}
      aria-label={`Page ${number}`}
      ref={(el) => {
        page.current = el
        ref(el)
      }}
    >
      <div className="pdfCanvas" ref={canvasHost} />
      <div className="textLayer" ref={textHost} />
    </div>
  )
}
