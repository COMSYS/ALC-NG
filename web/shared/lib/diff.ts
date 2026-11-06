// Line diff for the review: Myers' O(ND) algorithm in linear space, paired
// into side-by-side rows, segmented into hunks with collapsible unchanged
// runs (VS Code style), plus an intra-line word diff for changed pairs.

export const DIFF_CONTEXT = 3  // context lines around each change (VS Code default)

export type LineOp = { t: 'eq'; a: number; b: number } | { t: 'del'; a: number } | { t: 'ins'; b: number }

export type DiffRow = { kind: 'eq'; a: number; b: number } | { kind: 'pair'; a: number | null; b: number | null }

export type DiffSegment = { t: 'rows' | 'bar'; from: number; to: number }

// A run of text with an optional class (a word-diff mark or a TeX token).
export interface Span {
  text: string
  cls?: string
}

export function splitLines(text: string): string[] {
  if (!text) return []
  const lines = text.replace(/\r\n?/g, '\n').split('\n')
  if (lines.length && lines[lines.length - 1] === '') lines.pop()
  return lines
}

// Myers middle snake (divide & conquer, linear space), operating on the line
// arrays. Returns the split point {x, y} — x into the A-range, y into the
// B-range — or null when the ranges share nothing.
function middleSnake(A: string[], a0: number, a1: number, B: string[], b0: number, b1: number) {
  const N = a1 - a0
  const M = b1 - b0
  if (N === 0 || M === 0) return null
  const maxD = Math.ceil((N + M) / 2)
  if (maxD > 30000) return null
  const off = maxD
  const len = 2 * maxD + 2
  const vf = new Int32Array(len).fill(-1)
  const vb = new Int32Array(len).fill(-1)
  vf[off + 1] = 0
  vb[off + 1] = 0
  const delta = N - M
  const front = delta % 2 !== 0  // odd delta: overlap is found on a forward step
  let k1start = 0
  let k1end = 0
  let k2start = 0
  let k2end = 0
  for (let d = 0; d <= maxD; d++) {
    for (let k = -d + k1start; k <= d - k1end; k += 2) {
      const i = off + k
      let x = k === -d || (k !== d && vf[i - 1] < vf[i + 1]) ? vf[i + 1] : vf[i - 1] + 1
      let y = x - k
      while (x < N && y < M && A[a0 + x] === B[b0 + y]) {
        x++
        y++
      }
      vf[i] = x
      if (x > N) k1end += 2
      else if (y > M) k1start += 2
      else if (front) {
        // Overlap: the backward path has crossed this forward diagonal
        // (internal index delta - k) and the two reaches overlap.
        const j = off + delta - k
        if (j >= 0 && j < len && vb[j] !== -1 && x + vb[j] >= N) return { x: a0 + x, y: b0 + y }
      }
    }
    for (let k = -d + k2start; k <= d - k2end; k += 2) {
      const i = off + k
      let x = k === -d || (k !== d && vb[i - 1] < vb[i + 1]) ? vb[i + 1] : vb[i - 1] + 1
      let y = x - k
      while (x < N && y < M && A[a1 - 1 - x] === B[b1 - 1 - y]) {
        x++
        y++
      }
      vb[i] = x
      if (x > N) k2end += 2
      else if (y > M) k2start += 2
      else if (!front) {
        const j = off + delta - k  // forward diagonal of this backward path
        if (j >= 0 && j < len && vf[j] !== -1 && vf[j] + x >= N) {
          const x1 = vf[j]
          const y1 = x1 - (delta - k)
          return { x: a0 + x1, y: b0 + y1 }
        }
      }
    }
  }
  return null
}

// The edit script from A to B, with running indices.
export function lineOps(A: string[], B: string[]): LineOp[] {
  const ops: LineOp[] = []
  const aLen = A.length
  const bLen = B.length
  let p = 0
  while (p < aLen && p < bLen && A[p] === B[p]) {
    ops.push({ t: 'eq', a: p, b: p })
    p++
  }
  let s = 0
  while (s < aLen - p && s < bLen - p && A[aLen - 1 - s] === B[bLen - 1 - s]) s++
  const aMid = aLen - s
  const bMid = bLen - s
  const recurse = (a0: number, a1: number, b0: number, b1: number) => {
    // Re-trim common prefix/suffix — without this, a half that is identical
    // to its counterpart recurses on itself forever.
    while (a0 < a1 && b0 < b1 && A[a0] === B[b0]) ops.push({ t: 'eq', a: a0++, b: b0++ })
    let sfx = 0
    while (a0 + sfx < a1 && b0 + sfx < b1 && A[a1 - 1 - sfx] === B[b1 - 1 - sfx]) sfx++
    a1 -= sfx
    b1 -= sfx
    if (a0 === a1) {
      for (; b0 < b1; b0++) ops.push({ t: 'ins', b: b0 })
    } else if (b0 === b1) {
      for (; a0 < a1; a0++) ops.push({ t: 'del', a: a0 })
    } else {
      const cut = middleSnake(A, a0, a1, B, b0, b1)
      if (cut) {
        recurse(a0, cut.x, b0, cut.y)
        recurse(cut.x, a1, cut.y, b1)
      } else {
        for (let i = a0; i < a1; i++) ops.push({ t: 'del', a: i })
        for (let j = b0; j < b1; j++) ops.push({ t: 'ins', b: j })
      }
    }
    for (let i = 0; i < sfx; i++) ops.push({ t: 'eq', a: a1 + i, b: b1 + i })
  }
  recurse(p, aMid, p, bMid)
  for (let i = 0; i < s; i++) ops.push({ t: 'eq', a: aMid + i, b: bMid + i })
  return ops
}

// Side-by-side rows: eq pairs align directly; any mixed del/ins run is zipped
// pairwise (the shorter side gets a hatched filler).
export function pairRows(ops: LineOp[]): DiffRow[] {
  const rows: DiffRow[] = []
  let i = 0
  while (i < ops.length) {
    const op = ops[i]
    if (op.t === 'eq') {
      rows.push({ kind: 'eq', a: op.a, b: op.b })
      i++
      continue
    }
    const dels: number[] = []
    const inss: number[] = []
    let j = i
    for (; j < ops.length; j++) {
      const run = ops[j]
      if (run.t === 'eq') break
      if (run.t === 'del') dels.push(run.a)
      else inss.push(run.b)
    }
    const n = Math.max(dels.length, inss.length)
    for (let k = 0; k < n; k++) rows.push({ kind: 'pair', a: dels[k] ?? null, b: inss[k] ?? null })
    i = j
  }
  return rows
}

// Hunk segmentation with collapsible runs of unchanged lines. Returns null
// when nothing changed at all.
export function hunkSegments(rows: DiffRow[], ctx = DIFF_CONTEXT): DiffSegment[] | null {
  const changed: number[] = []
  for (let i = 0; i < rows.length; i++) if (rows[i].kind === 'pair') changed.push(i)
  if (!changed.length) return null
  const windows: { s: number; e: number }[] = []
  for (const idx of changed) {
    const s = Math.max(0, idx - ctx)
    const e = Math.min(rows.length - 1, idx + ctx)
    const last = windows[windows.length - 1]
    if (last && s <= last.e + 1) last.e = e
    else windows.push({ s, e })
  }
  const segs: DiffSegment[] = []
  let prev = -1
  for (const w of windows) {
    if (w.s > prev + 1) segs.push({ t: 'bar', from: prev + 1, to: w.s - 1 })
    segs.push({ t: 'rows', from: w.s, to: w.e })
    prev = w.e
  }
  if (prev < rows.length - 1) segs.push({ t: 'bar', from: prev + 1, to: rows.length - 1 })
  return segs
}

// Intra-line word diff for paired lines: the changed middle of each side is
// marked. Returns null when impractical (no change, or very long lines).
export function intraLine(a: string | null, b: string | null): { a: Span[]; b: Span[] } | null {
  if (!a || !b || a === b || a.length + b.length > 700) return null
  const at = a.match(/\s+|\S+/g) ?? []
  const bt = b.match(/\s+|\S+/g) ?? []
  let p = 0
  while (p < at.length && p < bt.length && at[p] === bt[p]) p++
  let s = 0
  while (s < at.length - p && s < bt.length - p && at[at.length - 1 - s] === bt[bt.length - 1 - s]) s++
  const am = at.slice(p, at.length - s)
  const bm = bt.slice(p, bt.length - s)
  if (!am.length && !bm.length) return null
  if (am.length * bm.length > 900) return null
  const n = am.length
  const m = bm.length
  const dp = Array.from({ length: n + 1 }, () => new Uint16Array(m + 1))
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i][j] = am[i] === bm[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1])
    }
  }
  // Walks the tokens of one side against the other; `lcs(i, j)` is the LCS
  // length of tokens[i..] and other[j..] (the B side reads the table of A
  // against B transposed).
  const marked = (tokens: string[], other: string[], cls: string, lcs: (i: number, j: number) => number): Span[] => {
    const out: Span[] = []
    let i = 0
    let j = 0
    let mark = ''
    const flush = () => {
      if (mark) out.push({ text: mark, cls })
      mark = ''
    }
    while (i < tokens.length) {
      if (j < other.length && tokens[i] === other[j] && lcs(i, j) === lcs(i + 1, j + 1) + 1) {
        flush()
        out.push({ text: tokens[i] })
        i++
        j++
      } else {
        mark += tokens[i]
        i++
      }
    }
    flush()
    return out
  }
  const head = (tokens: string[]) => (p > 0 ? [{ text: tokens.slice(0, p).join('') }] : [])
  const tail = (tokens: string[]) => (s > 0 ? [{ text: tokens.slice(tokens.length - s).join('') }] : [])
  return {
    a: [...head(at), ...marked(am, bm, 'wdel', (i, j) => dp[i][j]), ...tail(at)],
    b: [...head(bt), ...marked(bm, am, 'wins', (i, j) => dp[j][i]), ...tail(bt)],
  }
}

// Cheap LaTeX decoration for unchanged context: comments and \commands.
export function texDecor(raw: string): Span[] {
  let cut = -1
  for (let i = 0; i < raw.length; i++) {
    if (raw[i] !== '%') continue
    let back = 0
    for (let j = i - 1; j >= 0 && raw[j] === '\\'; j--) back++
    if (back % 2 === 0) {
      cut = i
      break
    }
  }
  const head = cut >= 0 ? raw.slice(0, cut) : raw
  const spans: Span[] = []
  let last = 0
  for (const m of head.matchAll(/\\[A-Za-z@]+\*?/g)) {
    if (m.index > last) spans.push({ text: head.slice(last, m.index) })
    spans.push({ text: m[0], cls: 't-cmd' })
    last = m.index + m[0].length
  }
  if (last < head.length) spans.push({ text: head.slice(last) })
  if (cut >= 0) spans.push({ text: raw.slice(cut), cls: 't-c' })
  return spans
}
