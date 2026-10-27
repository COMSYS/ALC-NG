import { describe, expect, it } from 'vitest'
import { hunkSegments, intraLine, lineOps, pairRows, splitLines, texDecor, type LineOp } from './diff'

// Rebuilds both sides from an edit script, so a test can check that the
// script is a faithful description of A → B.
function replay(ops: LineOp[], A: string[], B: string[]) {
  const a: string[] = []
  const b: string[] = []
  for (const op of ops) {
    if (op.t !== 'ins') a.push(A[op.a])
    if (op.t !== 'del') b.push(B[op.b])
    if (op.t === 'eq') expect(A[op.a]).toBe(B[op.b])
  }
  return { a, b }
}

// Reference LCS length by dynamic programming.
function lcs(A: string[], B: string[]) {
  const dp = Array.from({ length: A.length + 1 }, () => new Array<number>(B.length + 1).fill(0))
  for (let i = A.length - 1; i >= 0; i--)
    for (let j = B.length - 1; j >= 0; j--) dp[i][j] = A[i] === B[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1])
  return dp[0][0]
}

// Deterministic pseudo-random line arrays over a small alphabet, so that
// equal lines are frequent.
function* cases(count: number) {
  let seed = 7
  const rand = () => ((seed = (seed * 1103515245 + 12345) % 2 ** 31) / 2 ** 31)
  for (let n = 0; n < count; n++) {
    const line = () => 'abcde'[Math.floor(rand() * 5)]
    yield [Array.from({ length: Math.floor(rand() * 14) }, line), Array.from({ length: Math.floor(rand() * 14) }, line)]
  }
}

describe('lineOps', () => {
  it('describes both sides and keeps a longest common subsequence', () => {
    for (const [A, B] of cases(400)) {
      const ops = lineOps(A, B)
      const { a, b } = replay(ops, A, B)
      expect(a).toEqual(A)
      expect(b).toEqual(B)
      expect(ops.filter((op) => op.t === 'eq').length).toBe(lcs(A, B))
    }
  })

  it('handles empty sides', () => {
    expect(lineOps([], [])).toEqual([])
    expect(lineOps(['x'], [])).toEqual([{ t: 'del', a: 0 }])
    expect(lineOps([], ['y'])).toEqual([{ t: 'ins', b: 0 }])
  })
})

describe('pairRows and hunkSegments', () => {
  it('zips a replaced run into pairs and folds distant context', () => {
    const A = splitLines('1\n2\n3\n4\n5\n6\n7\n8\n9\nold\n')
    const B = splitLines('1\n2\n3\n4\n5\n6\n7\n8\n9\nnew\nextra\n')
    const rows = pairRows(lineOps(A, B))
    expect(rows.slice(-2)).toEqual([
      { kind: 'pair', a: 9, b: 9 },
      { kind: 'pair', a: null, b: 10 },
    ])
    expect(hunkSegments(rows)).toEqual([
      { t: 'bar', from: 0, to: 5 },
      { t: 'rows', from: 6, to: 10 },
    ])
  })

  it('reports no hunks for identical text', () => {
    const lines = splitLines('a\r\nb\r\n')
    expect(lines).toEqual(['a', 'b'])
    expect(hunkSegments(pairRows(lineOps(lines, lines)))).toBeNull()
  })
})

describe('intraLine', () => {
  it('marks only the changed words on both sides', () => {
    const marks = intraLine('see the old result here', 'see the new result here')!
    const text = (spans: { text: string }[]) => spans.map((s) => s.text).join('')
    expect(text(marks.a)).toBe('see the old result here')
    expect(text(marks.b)).toBe('see the new result here')
    expect(marks.a.filter((s) => s.cls === 'wdel').map((s) => s.text)).toEqual(['old'])
    expect(marks.b.filter((s) => s.cls === 'wins').map((s) => s.text)).toEqual(['new'])
  })

  it('handles sides of different lengths', () => {
    const marks = intraLine('a b', 'a x y z b')!
    expect(marks.b.filter((s) => s.cls === 'wins').map((s) => s.text).join('')).toBe('x y z ')
    expect(intraLine('same', 'same')).toBeNull()
  })
})

describe('texDecor', () => {
  it('marks commands and an unescaped comment', () => {
    expect(texDecor('\\section{A} 50\\% done % note')).toEqual([
      { text: '\\section', cls: 't-cmd' },
      { text: '{A} 50\\% done ' },
      { text: '% note', cls: 't-c' },
    ])
  })
})
