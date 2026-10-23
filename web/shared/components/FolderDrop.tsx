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

// The submission drop zone: a whole folder, dropped or picked. The picker is
// a hidden <input webkitdirectory>, so a click anywhere on the zone opens it.
export function FolderDrop({ icon, folder, disabled, running }: Props) {
  const input = useRef<HTMLInputElement>(null)
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
      <span className="dropIcon">
        {icon}
      </span>
      <span className="dropText">
        <span className="dropTitle">{folder ? folder.label : 'Choose a folder'}</span>
        <span className="dropHint">
          {folder ? `${folder.count} ${plural(folder.count, 'file')}. Click or drop to replace.` : 'or drop the submission folder here'}
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
