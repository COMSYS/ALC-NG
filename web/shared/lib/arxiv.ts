import { unpackArchive, type Unpacked } from './archive'
import { fetchWithProgress } from './net'

// Fetching a submission straight from arXiv by its identifier: the fastest
// path to a cleaned archive for a paper that is already up there (a v2 is
// usually what an author is preparing), and the only way to try the tool on
// a real submission without handing over an unpublished one.
//
// export.arxiv.org answers `Access-Control-Allow-Origin: *`, so the browser
// does this itself — the bytes go from arXiv to the tab, not through our
// server.

const BASE = 'https://export.arxiv.org'

/** `/abs/<id>` on the main site, for linking the paper a fetch came from. */
export const arxivAbsUrl = (id: string) => `https://arxiv.org/abs/${id}`

/** The address the sources come from, also shown when a fetch cannot run. */
export const arxivSrcUrl = (id: string) => `${BASE}/src/${path(id)}`

const path = (id: string) => id.split('/').map(encodeURIComponent).join('/')

/**
 * The arXiv identifier in anything an author is likely to paste: a bare id
 * (`2303.12712`, `2303.12712v5`), the old scheme (`hep-th/9901001`), an
 * `arXiv:` prefix, or an abs/pdf/src URL. Returns null when it is none.
 */
export function parseArxivId(raw: string): string | null {
  const text = raw.trim().replace(/^arxiv[:\s]+/i, '')
  const url = /(?:^|\/\/|\.)arxiv\.org\/(?:abs|pdf|src|e-print|format)\/(.+)$/i.exec(text)
  const candidate = (url ? url[1] : text).replace(/[?#].*$/, '').replace(/\.pdf$/i, '').trim()
  const modern = /^(\d{4}\.\d{4,5})(v\d+)?$/.exec(candidate)
  const legacy = /^([a-z-]+(?:\.[A-Z]{2})?\/\d{7})(v\d+)?$/i.exec(candidate)
  const m = modern ?? legacy
  return m ? `${m[1]}${m[2] ?? ''}` : null
}

/**
 * Downloads the submitted sources of `id` and unpacks them. `onProgress` is
 * the byte counter of the download (see fetchWithProgress).
 */
export async function fetchArxivSources(id: string, onProgress: (loaded: number, total: number) => void): Promise<Unpacked> {
  // /e-print/<id> is arXiv's documented address, but it answers a 301 to
  // /src/<id> and that redirect carries no CORS header of its own, which a
  // browser rejects before it ever sees the real response. So address /src/
  // directly and keep /e-print/ as the fallback for the day that flips.
  let bytes: ArrayBuffer
  try {
    bytes = await fetchWithProgress(`${BASE}/src/${path(id)}`, onProgress)
  } catch (first) {
    try {
      bytes = await fetchWithProgress(`${BASE}/e-print/${path(id)}`, onProgress)
    } catch {
      throw explain(first as Error, id)
    }
  }

  const unpacked = await unpackArchive(id, bytes).catch((err: Error) => {
    if (/PDF/i.test(err.message)) {
      throw new Error(`arXiv has no LaTeX sources for ${id} — the submission is a PDF upload.`)
    }
    throw new Error(`the sources of ${id} could not be unpacked: ${err.message}`)
  })
  // a withdrawn submission is served as a single note instead of sources
  if (unpacked.files.length === 1 && /^removed\b/i.test(unpacked.files[0].rel)) {
    throw new Error(`arXiv serves no sources for ${id} — the submission was withdrawn.`)
  }
  // the label becomes the leading path segment of every file, so the slash
  // of a legacy identifier (hep-th/9901001) cannot stay in it
  return { ...unpacked, label: `arXiv-${id.replace(/\//g, '-')}` }
}

/** A download failure in the words of someone who can act on it. */
function explain(err: Error, id: string): Error {
  const status = /HTTP (\d{3})/.exec(err.message)?.[1]
  if (status === '404') return new Error(`arXiv has no submission ${id}.`)
  if (status === '403' || status === '429') {
    return new Error('arXiv is rate-limiting this address. Wait a minute and try again.')
  }
  return new Error(
    `the sources could not be downloaded from arXiv (${err.message}). ` +
      `Download ${arxivSrcUrl(id)} yourself and drop the file here.`,
  )
}
