import { useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent, type ReactNode } from 'react'
import { createPortal } from 'react-dom'

export interface SelectOption<V> {
  value: V
  label: string
  hint?: string
}

interface Props<V> {
  /** the open/close indicator, drawn by the design (tool/, site/) */
  icon: ReactNode
  value: V | null
  options: SelectOption<V>[]
  placeholder: string
  onChange: (value: V) => void
  disabled?: boolean
  loading?: boolean
  ariaLabel?: string
}

// A dropdown whose options carry a second line (the sample picker's blurbs),
// which a native <select> cannot show. The list is portaled to <body> so the
// scrolling rail does not clip it.
export function Select<V>({ icon, value, options, placeholder, onChange, disabled, loading, ariaLabel }: Props<V>) {
  const button = useRef<HTMLButtonElement>(null)
  const list = useRef<HTMLUListElement>(null)
  const [open, setOpen] = useState(false)
  const [active, setActive] = useState(0)
  const [rect, setRect] = useState<DOMRect | null>(null)
  const id = useId()
  const selected = options.findIndex((o) => o.value === value)

  const openList = () => {
    if (disabled) return
    setRect(button.current?.getBoundingClientRect() ?? null)
    setActive(Math.max(0, selected))
    setOpen(true)
  }
  const close = (focus = true) => {
    setOpen(false)
    if (focus) button.current?.focus()
  }
  const choose = (index: number) => {
    const option = options[index]
    close()
    if (option && option.value !== value) onChange(option.value)
  }

  // close on a click elsewhere, and when the page scrolls or resizes under it
  useEffect(() => {
    if (!open) return
    const onPointer = (ev: PointerEvent) => {
      const target = ev.target as Node
      if (!button.current?.contains(target) && !list.current?.contains(target)) close(false)
    }
    const onMove = (ev: Event) => {
      if (!list.current?.contains(ev.target as Node)) close(false)
    }
    document.addEventListener('pointerdown', onPointer)
    window.addEventListener('scroll', onMove, true)
    window.addEventListener('resize', onMove)
    return () => {
      document.removeEventListener('pointerdown', onPointer)
      window.removeEventListener('scroll', onMove, true)
      window.removeEventListener('resize', onMove)
    }
  }, [open])

  useLayoutEffect(() => {
    if (open) list.current?.focus()
  }, [open])

  const onButtonKey = (ev: KeyboardEvent) => {
    if (ev.key === 'ArrowDown' || ev.key === 'ArrowUp') {
      ev.preventDefault()
      openList()
    }
  }
  const onListKey = (ev: KeyboardEvent) => {
    if (ev.key === 'ArrowDown') setActive((i) => Math.min(options.length - 1, i + 1))
    else if (ev.key === 'ArrowUp') setActive((i) => Math.max(0, i - 1))
    else if (ev.key === 'Home') setActive(0)
    else if (ev.key === 'End') setActive(options.length - 1)
    else if (ev.key === 'Enter' || ev.key === ' ') choose(active)
    else if (ev.key === 'Escape' || ev.key === 'Tab') close(ev.key === 'Escape')
    else return
    ev.preventDefault()
  }

  const current = options[selected]
  return (
    <>
      <button
        ref={button}
        type="button"
        className={`select${open ? ' open' : ''}`}
        disabled={disabled}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        aria-label={ariaLabel}
        onClick={() => (open ? close() : openList())}
        onKeyDown={onButtonKey}
      >
        <span className={current ? 'selectValue' : 'selectPlaceholder'}>{current ? current.label : placeholder}</span>
        {loading ? <span className="spinner" aria-label="loading" /> : icon}
      </button>
      {open &&
        rect &&
        createPortal(
          <ul
            ref={list}
            id={id}
            role="listbox"
            tabIndex={-1}
            className="selectList"
            style={{ left: rect.left, top: rect.bottom + 4, width: rect.width }}
            aria-activedescendant={`${id}-${active}`}
            onKeyDown={onListKey}
          >
            {options.map((option, i) => (
              <li
                key={i}
                id={`${id}-${i}`}
                role="option"
                aria-selected={i === selected}
                className={`selectOption${i === active ? ' active' : ''}`}
                onPointerEnter={() => setActive(i)}
                onClick={() => choose(i)}
              >
                <span>{option.label}</span>
                {option.hint && <small>{option.hint}</small>}
              </li>
            ))}
          </ul>,
          document.body,
        )}
    </>
  )
}
