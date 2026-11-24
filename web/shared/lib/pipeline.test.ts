import { describe, expect, it } from 'vitest'
import { imageDiff, pagesEqual, parseHexColor } from './compare'
import { flsFromAccessLog, synthesizeFlsFromLog } from './compile'
import { documentClassOf, rankMainCandidates } from './folder'
import { normalizeAccessPath, stemOf, stripTexComments } from './paths'
import { computeStats, kindOf } from './review'
import type { Change, Review } from '../state/review'

describe('normalizeAccessPath', () => {
  it('maps engine opens onto submission paths', () => {
    expect(normalizeAccessPath('/work/main.tex')).toBe('main.tex')
    expect(normalizeAccessPath('./used.tex', 'paper')).toBe('paper/used.tex')
    expect(normalizeAccessPath('../figs/a.png', 'paper')).toBe('figs/a.png')
    expect(normalizeAccessPath('/tex/cmr10.tfm')).toBeNull()
    expect(normalizeAccessPath('../../outside.tex', 'paper')).toBeNull()
    expect(normalizeAccessPath(undefined)).toBeNull()
  })
})

describe('main file detection', () => {
  it('reads the document class, ignoring commented-out lines', () => {
    expect(documentClassOf('% \\documentclass{beamer}\n\\documentclass[11pt]{article}')).toBe('article')
    expect(documentClassOf('\\input{x}')).toBeNull()
  })

  it('ranks top-level conventional names before nested and standalone files', () => {
    const ranked = rankMainCandidates([
      { rel: 'figs/plot.tex', cls: 'standalone' },
      { rel: 'appendix.tex', cls: 'article' },
      { rel: 'sub/main.tex', cls: 'article' },
      { rel: 'main.tex', cls: 'article' },
    ])
    expect(ranked.map((c) => c.rel)).toEqual(['main.tex', 'appendix.tex', 'sub/main.tex', 'figs/plot.tex'])
  })

  it('strips comments but keeps escaped percent signs', () => {
    expect(stripTexComments('50\\% done % note')).toBe('50\\% done ')
    expect(stemOf('dir/paper.v2.tex')).toBe('dir/paper.v2')
  })
})

describe('recorder files', () => {
  it('turns the access log into relative INPUT/OUTPUT lines', () => {
    expect(flsFromAccessLog({ inputs: new Set(['main.tex', 'sub/a.tex']), outputs: new Set(['main.aux']) })).toBe(
      'INPUT ./main.tex\nINPUT ./sub/a.tex\nOUTPUT ./main.aux\n',
    )
  })

  it('falls back to the files a compile log mentions', () => {
    const fls = synthesizeFlsFromLog('(./main.tex (./used.tex) (/usr/share/texmf/article.cls))')
    expect(fls).toContain('INPUT ./main.tex')
    expect(fls).toContain('INPUT ./used.tex')
    expect(fls).toContain('OUTPUT ./main.pdf')
  })
})

describe('statistics', () => {
  const change = (rel: string, status: Change['status'], origSize: number | null, outSize: number | null): Change => ({
    rel,
    status,
    origSize,
    outSize,
    origFile: null,
    outPath: outSize === null ? null : `/cleaned/${rel}`,
    origText: null,
    newText: null,
  })

  it('splits the original bytes into dropped, trimmed and added', () => {
    const review: Review = {
      stats: { M: 1, D: 1, A: 1, R: 0, U: 1 },
      changes: [change('main.tex', 'M', 1000, 600), change('old.png', 'D', 5000, null), change('new.bbl', 'A', null, 200)],
      unchanged: [change('refs.bib', 'U', 300, 300)],
      archive: null,
    }
    const stats = computeStats(review, 1500)
    expect(stats).toMatchObject({
      origBytes: 6300,
      cleanedBytes: 1100,
      savedBytes: 5200,
      droppedBytes: 5000,
      trimmedBytes: 400,
      addedBytes: 200,
      origN: 3,
      outN: 3,
    })
    expect(stats.reductions.map((r) => r.rel)).toEqual(['old.png', 'main.tex'])
    expect(stats.kindRows[0]).toMatchObject({ kind: 'figures', origN: 1, outN: 0, removed: 5000 })
    expect(kindOf('x.PNG')).toBe('figures')
    expect(kindOf('Makefile')).toBe('other')
  })
})

describe('pixel compare', () => {
  const page = (width: number, height: number, fill: number) => ({ width, height, px: new Uint32Array(width * height).fill(fill) })

  it('paints changed pixels and keeps the identical ones', () => {
    const white = 0xffffffff
    const before = page(2, 1, white)
    const after = page(2, 1, white)
    after.px[1] = 0xff000000  // one black pixel
    expect(pagesEqual(before, after)).toBe(false)
    expect(pagesEqual(before, page(2, 1, white))).toBe(true)
    const diff = imageDiff(before, after, parseHexColor('#ff0000'))
    expect([...diff.rgba]).toEqual([0xffffffff, 0xff0000ff])
  })

  it('rejects malformed colours', () => {
    expect(parseHexColor('00ff00')).toEqual([0, 255, 0])
    expect(() => parseHexColor('#fff')).toThrow()
  })
})
