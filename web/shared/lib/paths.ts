export const dirOf = (rel: string) => (rel.includes('/') ? rel.slice(0, rel.lastIndexOf('/')) : '')

export const baseOf = (rel: string) => rel.slice(rel.lastIndexOf('/') + 1)

// path without its final extension
export const stemOf = (rel: string) => rel.replace(/\.[^./]*$/, '')

export const stripTexComments = (text: string) => text.replace(/(^|[^\\])%.*$/gm, '$1')

// Percent-encodes the segments of a URL path, keeping the separators: real
// submissions carry spaces and other surprises in their names ("Section 2/").
export const encPath = (path: string) => path.split('/').map(encodeURIComponent).join('/')

// one-line failure tail for the log
export const failTail = (log: unknown) => String(log ?? '').slice(-160)

// Normalizes a recorded open path to a submission-relative path. Relative
// paths are relative to the compile directory `dir` (the main file's).
// Returns null for other absolute paths (kpathsea cache, TeX distribution)
// and anything reaching outside the submission.
export function normalizeAccessPath(path: string | undefined, dir = ''): string | null {
  if (!path) return null
  let full: string
  if (path.startsWith('/work/')) full = path.slice('/work/'.length)
  else if (path.startsWith('/')) return null
  else full = dir ? `${dir}/${path}` : path
  const parts: string[] = []
  for (const part of full.split('/')) {
    if (part === '' || part === '.') continue
    if (part === '..') {
      if (parts.length === 0) return null
      parts.pop()
    } else {
      parts.push(part)
    }
  }
  return parts.length > 0 ? parts.join('/') : null
}
