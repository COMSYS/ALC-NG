import { logErr } from '../state/log'
import type { WorkerMessage, WorkerReply } from './engine'

export interface StagedFile {
  rel: string
  bytes: Uint8Array
}

export type WorkerCall = (message: WorkerMessage, responseCmds: string[] | null) => Promise<WorkerReply | undefined>

// Stage files into a worker's memfs: mkdir once per directory (the worker
// treats re-creation as a failure), then writefile. `call` speaks either
// worker's protocol; `onWrite(rel, bytes)` sees every written file and
// `label` names the worker in staging errors.
export async function stageIntoEngine(
  entries: StagedFile[],
  call: WorkerCall,
  onWrite?: ((rel: string, bytes: Uint8Array) => void) | null,
  label = 'engine',
) {
  const madeDirs = new Set<string>()
  for (const { rel, bytes } of entries) {
    let prefix = ''
    for (const part of rel.split('/').slice(0, -1)) {
      prefix = prefix ? `${prefix}/${part}` : part
      if (!madeDirs.has(prefix)) {
        await call({ cmd: 'mkdir', url: prefix }, ['mkdir'])
        madeDirs.add(prefix)
      }
    }
    const reply = await call({ cmd: 'writefile', url: rel, src: bytes }, ['writefile'])
    if (reply?.result !== undefined && reply.result !== 'ok') logErr(`[${label}] could not stage ${rel}`)
    onWrite?.(rel, bytes)
  }
}
