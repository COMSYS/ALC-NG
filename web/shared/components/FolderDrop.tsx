import { useEffect, useRef, useState, type DragEvent, type ReactNode } from 'react'
import { clearFolder, setFolderFromDrop, setFolderFromInput } from '../lib/folder'
import { plural } from '../lib/format'
import { logErr } from '../state/log'

interface Props {
  /** drawn by the design (tool/, site/) */
  icon: ReactNode
  folder: { label: string; count: number } | null
  disabled: boolean
  running: boolean
}

// The submission drop zone: a whole folder, or the archive it usually comes
// in (Overleaf exports a .zip, arXiv a .tar.gz) — dropped or picked. The
// folder picker is a hidden <input webkitdirectory>, so a click anywhere on
// the zone opens it; the archive picker is a second input, because one input
// cannot offer both.
export function FolderDrop({ icon, folder, disabled, running }: Props) {
  const input = useRef<HTMLInputElement>(null)
  const archive = useRef<HTMLInputElement>(null)
  const [over, setOver] = useState(false)

  // React does not know the (non-standard) directory attributes
  useEffect(() => {
    input.current?.setAttribute('webkitdirectory', '')
    input.current?.setAttribute('directory', '')
  }, [])

  const onDragOver = (ev: DragEvent) => {
    if (disabled) return
    ev.preventDefault()
    ev.dataTransfer.dropEffect = 'copy'
    setOver(true)
  }
  const onDrop = (ev: DragEvent) => {
    ev.preventDefault()
    setOver(false)
    if (disabled) return
    setFolderFromDrop(ev.dataTransfer).catch((err: Error) => logErr(`could not read the dropped folder: ${err.message}`))
  }
  const pick = (ev: { preventDefault: () => void; stopPropagation: () => void }) => {
    // inside the <label>, so the folder input would open on the way out
    ev.preventDefault()
    ev.stopPropagation()
    archive.current?.click()
  }

  return (
    <label
      className={`drop${over ? ' over' : ''}${disabled ? ' disabled' : ''}`}
      onDragOver={onDragOver}
      onDragLeave={() => setOver(false)}
      onDrop={onDrop}
    >
      <input
        ref={input}
        type="file"
        multiple
        hidden
        disabled={disabled}
        onChange={(ev) => {
          if (ev.target.files) setFolderFromInput(ev.target.files)
          ev.target.value = ''  // picking the same folder again still reloads it
        }}
      />
      <input
        ref={archive}
        type="file"
        hidden
        disabled={disabled}
        accept=".zip,.tar,.tar.gz,.tgz,.gz,application/zip,application/gzip,application/x-tar"
        onChange={(ev) => {
          if (ev.target.files) setFolderFromInput(ev.target.files)
          ev.target.value = ''
        }}
      />
      <span className="dropIcon">
        {icon}
      </span>
      <span className="dropText">
        <span className="dropTitle">{folder ? folder.label : 'Choose a folder'}</span>
        <span className="dropHint">
          {folder ? (
            `${folder.count} ${plural(folder.count, 'file')}. Click or drop to replace.`
          ) : (
            <>
              drop it here, or{' '}
              <button type="button" className="linkButton" disabled={disabled} onClick={pick}>
                pick a .zip / .tar.gz
              </button>
            </>
          )}
        </span>
      </span>
      {folder && (
        <button
          type="button"
          className="linkButton"
          disabled={running}
          onClick={(ev) => {
            ev.preventDefault()
            ev.stopPropagation()
            clearFolder()
          }}
        >
          Clear
        </button>
      )}
    </label>
  )
}
