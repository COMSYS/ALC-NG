import { describe, expect, it } from 'vitest'
import { archiveLabel, looksLikeArchive, unpackArchive } from './archive'

// The archives are built here rather than committed as binaries, so the
// fixtures stay readable and a reader change is tested against the format,
// not against one recorded file.

const enc = new TextEncoder()
const dec = new TextDecoder()
const bytes = (text: string) => enc.encode(text)

interface Entry {
  name: string
  text: string
}

const FILES: Entry[] = [
  { name: 'main.tex', text: '\\documentclass{article}\n\\begin{document}hi\\end{document}\n' },
  { name: 'unused.tex', text: 'nobody includes this\n' },
  { name: 'figs/plot.pgf', text: '% a figure\n' },
]

// --- tar --------------------------------------------------------------------

const BLOCK = 512

function tarHeader(name: string, size: number, type: string): Uint8Array {
  const head = new Uint8Array(BLOCK)
  const put = (at: number, text: string) => head.set(enc.encode(text), at)
  const octal = (n: number, width: number) => n.toString(8).padStart(width - 1, '0') + '\0'

  let short = name
  let prefix = ''
  if (name.length > 100) {
    // ustar splits a long path into prefix + '/' + name
    const cut = name.indexOf('/', Math.max(0, name.length - 100))
    if (cut < 0 || cut > 155) throw new Error(`cannot split ${name} for a ustar header`)
    prefix = name.slice(0, cut)
    short = name.slice(cut + 1)
  }
  put(0, short)
  put(100, octal(0o644, 8))
  put(108, octal(0, 8))
  put(116, octal(0, 8))
  put(124, octal(size, 12))
  put(136, octal(0, 12))
  put(148, '        ')  // the checksum counts itself as spaces
  put(156, type)
  put(257, 'ustar\0')
  put(263, '00')
  put(345, prefix)
  const sum = head.reduce((a, b) => a + b, 0)
  put(148, `${sum.toString(8).padStart(6, '0')}\0 `)
  return head
}

function tar(entries: { name: string; data: Uint8Array; type?: string }[]): Uint8Array {
  const parts: Uint8Array[] = []
  for (const { name, data, type } of entries) {
    parts.push(tarHeader(name, data.length, type ?? '0'), data)
    const pad = (BLOCK - (data.length % BLOCK)) % BLOCK
    if (pad) parts.push(new Uint8Array(pad))
  }
  parts.push(new Uint8Array(2 * BLOCK))  // the two zero blocks that end a tar
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0))
  let at = 0
  for (const part of parts) {
    out.set(part, at)
    at += part.length
  }
  return out
}

const plainTar = (files: Entry[] = FILES) => tar(files.map((f) => ({ name: f.name, data: bytes(f.text) })))

async function compress(input: Uint8Array, format: 'gzip' | 'deflate-raw'): Promise<Uint8Array> {
  const stream = new Blob([input as BlobPart]).stream().pipeThrough(new CompressionStream(format))
  return new Uint8Array(await new Response(stream).arrayBuffer())
}

// --- zip --------------------------------------------------------------------

const CRC_TABLE = Array.from({ length: 256 }, (_, n) => {
  let c = n
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1
  return c >>> 0
})

const crc32 = (data: Uint8Array) =>
  (data.reduce((c, b) => CRC_TABLE[(c ^ b) & 0xff] ^ (c >>> 8), 0xffffffff) ^ 0xffffffff) >>> 0

async function zip(files: Entry[], { store = false } = {}): Promise<Uint8Array> {
  const locals: Uint8Array[] = []
  const central: Uint8Array[] = []
  let offset = 0

  for (const file of files) {
    const raw = bytes(file.text)
    const data = store ? raw : await compress(raw, 'deflate-raw')
    const name = enc.encode(file.name)
    const method = store ? 0 : 8

    const local = new Uint8Array(30 + name.length)
    const lv = new DataView(local.buffer)
    lv.setUint32(0, 0x04034b50, true)
    lv.setUint16(4, 20, true)
    lv.setUint16(8, method, true)
    lv.setUint32(14, crc32(raw), true)
    lv.setUint32(18, data.length, true)
    lv.setUint32(22, raw.length, true)
    lv.setUint16(26, name.length, true)
    local.set(name, 30)
    locals.push(local, data)

    const cen = new Uint8Array(46 + name.length)
    const cv = new DataView(cen.buffer)
    cv.setUint32(0, 0x02014b50, true)
    cv.setUint16(4, 20, true)
    cv.setUint16(6, 20, true)
    cv.setUint16(10, method, true)
    cv.setUint32(16, crc32(raw), true)
    cv.setUint32(20, data.length, true)
    cv.setUint32(24, raw.length, true)
    cv.setUint16(28, name.length, true)
    cv.setUint32(42, offset, true)
    cen.set(name, 46)
    central.push(cen)
    offset += local.length + data.length
  }

  const centralSize = central.reduce((n, c) => n + c.length, 0)
  const eocd = new Uint8Array(22)
  const ev = new DataView(eocd.buffer)
  ev.setUint32(0, 0x06054b50, true)
  ev.setUint16(8, files.length, true)
  ev.setUint16(10, files.length, true)
  ev.setUint32(12, centralSize, true)
  ev.setUint32(16, offset, true)

  const parts = [...locals, ...central, eocd]
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0))
  let at = 0
  for (const part of parts) {
    out.set(part, at)
    at += part.length
  }
  return out
}

/** The unpacked result as `path → text`, which is what the assertions read. */
const contents = (files: { rel: string; bytes: Uint8Array }[]) =>
  Object.fromEntries(files.map((f) => [f.rel, dec.decode(f.bytes)]))

const EXPECTED = Object.fromEntries(FILES.map((f) => [f.name, f.text]))

describe('unpackArchive', () => {
  it('reads a tar', async () => {
    const { files } = await unpackArchive('submission.tar', plainTar())
    expect(contents(files)).toEqual(EXPECTED)
  })

  it('reads a gzipped tar', async () => {
    const { files, label } = await unpackArchive('submission.tar.gz', await compress(plainTar(), 'gzip'))
    expect(contents(files)).toEqual(EXPECTED)
    expect(label).toBe('submission')
  })

  it('reads a deflated zip and a stored zip alike', async () => {
    const deflated = await unpackArchive('a.zip', await zip(FILES))
    const stored = await unpackArchive('b.zip', await zip(FILES, { store: true }))
    expect(contents(deflated.files)).toEqual(EXPECTED)
    expect(contents(stored.files)).toEqual(EXPECTED)
  })

  it('recognizes the format from the bytes, not the name', async () => {
    // arXiv serves /src/<id> with no file name extension at all
    const { files } = await unpackArchive('2303.12712', await compress(plainTar(), 'gzip'))
    expect(contents(files)).toEqual(EXPECTED)
  })

  it('drops the one folder a submission is wrapped in', async () => {
    const wrapped = FILES.map((f) => ({ ...f, name: `my-paper/${f.name}` }))
    const { files } = await unpackArchive('my-paper.zip', await zip(wrapped))
    expect(contents(files)).toEqual(EXPECTED)
  })

  it('keeps top-level files when there is no single wrapper', async () => {
    const mixed = [...FILES, { name: 'notes/todo.txt', text: 'later\n' }]
    const { files } = await unpackArchive('mixed.zip', await zip(mixed))
    expect(Object.keys(contents(files)).sort()).toEqual(['figs/plot.pgf', 'main.tex', 'notes/todo.txt', 'unused.tex'])
  })

  it('leaves out the noise a macOS zip carries', async () => {
    const noisy = [...FILES, { name: '__MACOSX/._main.tex', text: 'x' }, { name: '.DS_Store', text: 'y' }]
    const { files } = await unpackArchive('noisy.zip', await zip(noisy))
    expect(contents(files)).toEqual(EXPECTED)
  })

  // the root file keeps the wrapper-stripping above out of these two: what
  // they check is that a long path survives the header format it needs
  it('reads a path too long for the classic tar header', async () => {
    const long = `figures/${'a-rather-long-directory-name/'.repeat(4)}plot.png`
    expect(long.length).toBeGreaterThan(100)
    const archive = tar([
      { name: 'main.tex', data: bytes('root\n') },
      { name: long, data: bytes('PNG') },
    ])
    const { files } = await unpackArchive('long.tar', archive)
    expect(contents(files)).toEqual({ 'main.tex': 'root\n', [long]: 'PNG' })
  })

  it('reads a GNU long name entry', async () => {
    const long = `${'nested/'.repeat(20)}deep.tex`
    const archive = tar([
      { name: 'main.tex', data: bytes('root\n') },
      { name: '././@LongLink', data: bytes(`${long}\0`), type: 'L' },
      { name: long.slice(0, 99), data: bytes('deep'), type: '0' },
    ])
    const { files } = await unpackArchive('gnu.tar', archive)
    expect(contents(files)).toEqual({ 'main.tex': 'root\n', [long]: 'deep' })
  })

  it('takes a single gzipped file as the main source', async () => {
    const one = await unpackArchive('2303.12712', await compress(bytes('\\documentclass{article}\n'), 'gzip'))
    expect(contents(one.files)).toEqual({ 'main.tex': '\\documentclass{article}\n' })
    const named = await unpackArchive('paper.tex.gz', await compress(bytes('hi\n'), 'gzip'))
    expect(contents(named.files)).toEqual({ 'paper.tex': 'hi\n' })
  })

  it('says what a PDF is instead of failing obscurely', async () => {
    await expect(unpackArchive('paper.pdf', bytes('%PDF-1.7\n...'))).rejects.toThrow(/PDF/)
  })

  it('refuses what is not an archive at all', async () => {
    await expect(unpackArchive('notes.txt', bytes('just text'))).rejects.toThrow(/not a \.zip/)
  })
})

describe('looksLikeArchive', () => {
  it('knows the names a submission arrives under', () => {
    for (const name of ['a.zip', 'a.tar', 'a.tar.gz', 'a.tgz', 'a.tex.gz', 'A.TAR.GZ']) {
      expect(looksLikeArchive(name)).toBe(true)
    }
    for (const name of ['main.tex', 'figure.png', 'paper.pdf', 'archive']) {
      expect(looksLikeArchive(name)).toBe(false)
    }
  })

  it('names the submission after the archive', () => {
    expect(archiveLabel('my-paper.tar.gz')).toBe('my-paper')
    expect(archiveLabel('/downloads/arXiv-2303.12712v5.zip')).toBe('arXiv-2303.12712v5')
  })
})
