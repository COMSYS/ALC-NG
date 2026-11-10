import { describe, expect, it } from 'vitest'
import { parseArxivId } from './arxiv'

// What an author actually pastes into the field: the identifier as printed
// on the paper, with or without a version, as an arXiv: reference, or as the
// URL of the page they are looking at.
describe('parseArxivId', () => {
  it('takes the forms an identifier is written in', () => {
    const same: [string, string][] = [
      ['2303.12712', '2303.12712'],
      ['2303.12712v5', '2303.12712v5'],
      ['  1706.03762  ', '1706.03762'],
      ['arXiv:1706.03762', '1706.03762'],
      ['arXiv: 1706.03762v7', '1706.03762v7'],
      // the pre-2015 scheme, four digits after the dot
      ['0704.0001', '0704.0001'],
      // and the one before that, with an archive and an optional subclass
      ['hep-th/9901001', 'hep-th/9901001'],
      ['cond-mat/0101001', 'cond-mat/0101001'],
      ['math.AG/0601001v2', 'math.AG/0601001v2'],
    ]
    for (const [input, id] of same) expect(parseArxivId(input)).toBe(id)
  })

  it('takes the URL of the paper', () => {
    const urls: [string, string][] = [
      ['https://arxiv.org/abs/2303.12712', '2303.12712'],
      ['https://arxiv.org/abs/2303.12712v5', '2303.12712v5'],
      ['http://arxiv.org/pdf/2303.12712v5.pdf', '2303.12712v5'],
      ['arxiv.org/abs/2303.12712?context=cs.CL', '2303.12712'],
      ['https://export.arxiv.org/src/1706.03762', '1706.03762'],
      ['https://arxiv.org/abs/hep-th/9901001', 'hep-th/9901001'],
    ]
    for (const [input, id] of urls) expect(parseArxivId(input)).toBe(id)
  })

  it('refuses what is not an identifier', () => {
    for (const input of ['', 'hello', '12.345', '2303.123', 'https://example.com/abs/2303.12712', 'https://arxiv.org/list/cs.CL/recent']) {
      expect(parseArxivId(input)).toBeNull()
    }
  })
})
