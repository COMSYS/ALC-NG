// Unpacking submissions in the browser: .zip, .tar, .tar.gz/.tgz and a bare
// .gz, with no dependency — the streams come from DecompressionStream, which
// every engine that can run the LaTeX wasm has.
//
// This is how an arXiv submission is actually held: Overleaf hands out a
// .zip, arXiv hands out (and takes back) a .tar.gz. Asking for a folder and
// nothing else turned the most common starting point into an error message.

export interface UnpackedFile {
  /** path inside the archive, with the redundant top folder stripped */
  rel: string
  bytes: Uint8Array
}

export interface Unpacked {
  files: UnpackedFile[]
  /** the archive's own name, minus its extensions — the submission's label */
  label: string
}

const ARCHIVE_RE = /\.(zip|tar|tar\.gz|tgz|tar\.bz2|tbz2?|tar\.xz|txz|gz)$/i
const SUPPORTED_RE = /\.(zip|tar|tar\.gz|tgz|gz)$/i

/** Whether a name looks like an archive — including the ones we cannot read. */
export const looksLikeArchive = (name: string) => ARCHIVE_RE.test(name)

/** The name without its archive extensions, as the submission's label. */
export function archiveLabel(name: string): string {
  const base = name.split(/[\\/]/).pop() ?? name
  return base.replace(ARCHIVE_RE, '') || 'submission'
}

const dec = new TextDecoder()
const utf8 = (bytes: Uint8Array) => dec.decode(bytes)

function decompress(bytes: Uint8Array, format: 'gzip' | 'deflate-raw'): Promise<Uint8Array> {
  if (typeof DecompressionStream === 'undefined') {
    throw new Error('this browser cannot unpack compressed archives (no DecompressionStream)')
  }
  const stream = new Blob([bytes as BlobPart]).stream().pipeThrough(new DecompressionStream(format))
  return new Response(stream).arrayBuffer().then((buf) => new Uint8Array(buf))
}

const isGzip = (b: Uint8Array) => b.length > 2 && b[0] === 0x1f && b[1] === 0x8b
const isZip = (b: Uint8Array) => b.length > 4 && b[0] === 0x50 && b[1] === 0x4b && (b[2] === 3 || b[2] === 5 || b[2] === 7)
const isPdf = (b: Uint8Array) => b.length > 4 && b[0] === 0x25 && b[1] === 0x50 && b[2] === 0x44 && b[3] === 0x46
// a tar names its format at byte 257 ("ustar"); the GNU variant writes
// "ustar  \0" there, the POSIX one "ustar\000"
const isTar = (b: Uint8Array) => b.length > 262 && utf8(b.subarray(257, 262)) === 'ustar'

/**
 * Reads an archive into the files of one submission. `name` only names the
 * result and picks the format when the bytes do not say it themselves.
 */
export async function unpackArchive(name: string, input: ArrayBuffer | Uint8Array): Promise<Unpacked> {
  let bytes = input instanceof Uint8Array ? input : new Uint8Array(input)
  if (isPdf(bytes)) throw new Error('this is a PDF, not a source archive')
  if (!SUPPORTED_RE.test(name) && !isGzip(bytes) && !isZip(bytes) && !isTar(bytes)) {
    throw new Error(`${name}: not a .zip or .tar.gz archive`)
  }

  const label = archiveLabel(name)
  if (isZip(bytes)) return { label, files: strip(await unzip(bytes)) }

  const inner = (name.split(/[\\/]/).pop() ?? name).replace(/\.gz$/i, '')
  if (isGzip(bytes)) bytes = await decompress(bytes, 'gzip')
  if (isTar(bytes)) return { label, files: strip(untar(bytes)) }
  // A single gzipped file, which is exactly how arXiv serves a one-file
  // submission. Only its own name can say what it is; everything else is
  // the submission's main .tex by assumption.
  return { label, files: [{ rel: /\.tex$/i.test(inner) ? inner : 'main.tex', bytes }] }
}

// macOS zips carry a shadow tree of resource forks; it is noise in every
// submission and would only show up as files the compile never reads.
const isNoise = (rel: string) => rel.startsWith('__MACOSX/') || rel.split('/').pop() === '.DS_Store'

/**
 * Drops a single redundant top-level folder ("project/main.tex" →
 * "main.tex"), which is how Overleaf and arXiv wrap a submission. Nothing is
 * stripped when the entries do not all sit under the same one.
 */
function strip(files: UnpackedFile[]): UnpackedFile[] {
  const kept = files.filter((f) => f.rel && !isNoise(f.rel))
  if (kept.length === 0) throw new Error('the archive holds no files')
  const [first] = kept[0].rel.split('/')
  const shared = kept.every((f) => f.rel.startsWith(`${first}/`))
  return shared ? kept.map((f) => ({ ...f, rel: f.rel.slice(first.length + 1) })) : kept
}

// --- zip -------------------------------------------------------------------
//
// Read through the central directory at the end of the file (the only place
// a zip's own index is authoritative), then take each entry's bytes from its
// local header. Stored and deflated entries are what writers produce.

const EOCD_SIG = 0x06054b50
const CEN_SIG = 0x02014b50
const LOC_SIG = 0x04034b50

async function unzip(bytes: Uint8Array): Promise<UnpackedFile[]> {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  const u16 = (at: number) => view.getUint16(at, true)
  const u32 = (at: number) => view.getUint32(at, true)

  // the end-of-central-directory record sits in the last 64 KiB (its own
  // 22 bytes plus a comment of at most 0xffff)
  let eocd = -1
  for (let at = bytes.length - 22; at >= 0 && at > bytes.length - 22 - 0xffff; at--) {
    if (u32(at) === EOCD_SIG) {
      eocd = at
      break
    }
  }
  if (eocd < 0) throw new Error('not a zip file (no end-of-central-directory record)')
  const count = u16(eocd + 10)
  let at = u32(eocd + 16)
  if (at === 0xffffffff || count === 0xffff) throw new Error('zip64 archives are not supported')

  const out: UnpackedFile[] = []
  for (let i = 0; i < count; i++) {
    if (u32(at) !== CEN_SIG) throw new Error('damaged zip (central directory entry expected)')
    const method = u16(at + 10)
    const compressed = u32(at + 20)
    const size = u32(at + 24)
    const nameLen = u16(at + 28)
    const extraLen = u16(at + 30)
    const commentLen = u16(at + 32)
    const local = u32(at + 42)
    const rel = utf8(bytes.subarray(at + 46, at + 46 + nameLen))
    at += 46 + nameLen + extraLen + commentLen

    if (rel.endsWith('/')) continue  // a directory entry carries no data
    if (compressed === 0xffffffff || size === 0xffffffff) throw new Error(`${rel}: zip64 entries are not supported`)
    if (u32(local) !== LOC_SIG) throw new Error(`${rel}: damaged zip (local header expected)`)
    const start = local + 30 + u16(local + 26) + u16(local + 28)
    const raw = bytes.subarray(start, start + compressed)
    if (method === 0) out.push({ rel, bytes: raw })
    else if (method === 8) out.push({ rel, bytes: await decompress(raw, 'deflate-raw') })
    else throw new Error(`${rel}: unsupported zip compression (method ${method})`)
  }
  return out
}

// --- tar -------------------------------------------------------------------
//
// 512-byte header blocks, each followed by the file's bytes padded to the
// next block. GNU long names ('L') and pax headers ('x') prepend the real
// path as a data block of their own.

const BLOCK = 512

function octal(field: Uint8Array): number {
  const text = utf8(field).replace(/\0.*$/, '').trim()
  return text === '' ? 0 : parseInt(text, 8) || 0
}

function untar(bytes: Uint8Array): UnpackedFile[] {
  const out: UnpackedFile[] = []
  let longName: string | null = null

  for (let at = 0; at + BLOCK <= bytes.length; ) {
    const head = bytes.subarray(at, at + BLOCK)
    if (head.every((b) => b === 0)) break  // the two zero blocks that end a tar
    const name = utf8(head.subarray(0, 100)).replace(/\0.*$/, '')
    const size = octal(head.subarray(124, 136))
    const type = String.fromCharCode(head[156] || 0x30)
    const prefix = utf8(head.subarray(345, 500)).replace(/\0.*$/, '')
    const data = bytes.subarray(at + BLOCK, at + BLOCK + size)
    at += BLOCK + Math.ceil(size / BLOCK) * BLOCK

    if (type === 'L') {
      longName = utf8(data).replace(/\0.*$/, '')
      continue
    }
    if (type === 'x' || type === 'g') {
      // pax record: "<len> path=<value>\n"
      const path = /(?:^|\n)\d+ path=([^\n]*)/.exec(utf8(data))
      if (path && type === 'x') longName = path[1]
      continue
    }
    const rel = longName ?? (prefix ? `${prefix}/${name}` : name)
    longName = null
    if (type === '0' || type === '\0' || type === '7') out.push({ rel, bytes: data })
  }
  if (out.length === 0) throw new Error('the archive holds no files')
  return out
}
