export const fmtKB = (n: number) => `${(n / 1024).toFixed(1)} KB`

export const fmtSize = (n: number) => (n < 1024 ? `${n} B` : fmtKB(n))

export const fmtDelta = (a: number, b: number) =>
  b === a ? '±0 B' : `${b > a ? '+' : '−'}${fmtSize(Math.abs(b - a))}`

export const fmtDur = (ms: number) =>
  ms < 950 ? `${Math.max(0, Math.round(ms))} ms` : `${(ms / 1000).toFixed(1)} s`

// Percent once the total is known, transferred bytes while it is not.
export const dlNote = (loaded: number, total: number) =>
  total > 0 ? `${Math.floor((loaded / total) * 100)} %` : fmtSize(loaded)

export const plural = (n: number, word: string) => `${word}${n === 1 ? '' : 's'}`
