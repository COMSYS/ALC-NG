// GET with download progress: `onProgress(loadedBytes, totalBytes)` fires
// whenever the whole percentage moves (totalBytes is 0 when the server sent
// no Content-Length; then every event reports the running size). XHR,
// because fetch exposes no receive progress. Rejects on network trouble and
// on non-2xx.
export function fetchWithProgress(
  url: string,
  onProgress: (loaded: number, total: number) => void,
): Promise<ArrayBuffer> {
  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest()
    xhr.open('GET', url)
    xhr.responseType = 'arraybuffer'
    // null: nothing reported yet, so even an unknown total reports once
    let lastPct: number | null = null
    xhr.onprogress = (ev) => {
      const total = ev.lengthComputable ? ev.total : 0
      if (total > 0) {
        const pct = Math.floor((ev.loaded / total) * 100)
        if (pct === lastPct) return
        lastPct = pct
      }
      onProgress(ev.loaded, total)
    }
    xhr.onload = () => {
      if (xhr.status < 200 || xhr.status >= 300) {
        reject(new Error(`HTTP ${xhr.status} ${xhr.statusText}`))
        return
      }
      // progress events are coalesced and there is no guaranteed final one —
      // emit the full count so the caller's percentage never sticks short of
      // 100 % (a cache-served body delivers few events, then jumps to load)
      const body = xhr.response as ArrayBuffer | null
      const n = body ? body.byteLength : 0
      if (n > 0) onProgress(n, n)
      resolve(body ?? new ArrayBuffer(0))
    }
    xhr.onerror = () => reject(new Error('network error'))
    xhr.send()
  })
}

// Blob → invisible <a> → click → delayed revoke.
export function saveBlob(bytes: BlobPart, name: string) {
  const url = URL.createObjectURL(new Blob([bytes]))
  const link = document.createElement('a')
  link.href = url
  link.download = name
  link.click()
  setTimeout(() => URL.revokeObjectURL(url), 5000)
}
