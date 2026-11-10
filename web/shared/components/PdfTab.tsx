import { useStore } from '../lib/store'
import { uiStore } from '../state/ui'
import { PdfViewer } from './PdfViewer'

// The compiled PDF of the last run.
export function PdfTab() {
  const pdf = useStore(uiStore, (s) => s.pdf)
  if (!pdf) return null
  return <PdfViewer key={pdf.url} bytes={pdf.bytes} url={pdf.url} />
}
