'use client'

import { useEffect, useRef } from 'react'
import { createPortal } from 'react-dom'

import { sampleCanvasColor, useColorPickStore } from '@/lib/colorPick'

// Hides dialogs and popovers while picking so the page behind them can be
// clicked; they stay open and come back with the picked colour.
const HIDE_OVERLAYS_CSS = `
[role="dialog"], [data-slot="dialog-overlay"], [data-slot="sheet-overlay"],
[data-radix-popper-content-wrapper] { visibility: hidden !important; }
`

/** Full-screen click catcher shown while a colour picker waits for a pick. */
export function ScreenColorPicker() {
  const picking = useColorPickStore((s) => s.onPick !== null)
  const catcherRef = useRef<HTMLDivElement | null>(null)

  useEffect(() => {
    if (!picking) return
    const el = catcherRef.current
    // Native listeners stop the events before the document-level handlers
    // that would dismiss the hidden dialog / popover. The pick is applied on
    // the click that ends the press, so no event reaches the page below once
    // the catcher is gone.
    let picked: string | null = null
    let fallback: ReturnType<typeof setTimeout> | undefined
    const stop = (e: Event) => {
      e.preventDefault()
      e.stopPropagation()
    }
    const apply = () => {
      clearTimeout(fallback)
      const hex = picked
      picked = null
      if (hex) useColorPickStore.getState().finish(hex)
    }
    const onPointerUp = (e: PointerEvent) => {
      stop(e)
      if ((e.button ?? 0) !== 0) return
      picked = sampleCanvasColor(e.clientX, e.clientY)
      fallback = setTimeout(apply, 300)
    }
    const onClick = (e: MouseEvent) => {
      stop(e)
      apply()
    }
    const onContextMenu = (e: MouseEvent) => {
      stop(e)
      useColorPickStore.getState().cancel()
    }
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      e.preventDefault()
      e.stopImmediatePropagation()
      useColorPickStore.getState().cancel()
    }
    el?.addEventListener('pointerdown', stop)
    el?.addEventListener('mousedown', stop)
    el?.addEventListener('pointerup', onPointerUp)
    el?.addEventListener('click', onClick)
    el?.addEventListener('contextmenu', onContextMenu)
    window.addEventListener('keydown', onKeyDown, true)
    return () => {
      clearTimeout(fallback)
      el?.removeEventListener('pointerdown', stop)
      el?.removeEventListener('mousedown', stop)
      el?.removeEventListener('pointerup', onPointerUp)
      el?.removeEventListener('click', onClick)
      el?.removeEventListener('contextmenu', onContextMenu)
      window.removeEventListener('keydown', onKeyDown, true)
    }
  }, [picking])

  if (!picking) return null
  return createPortal(
    <>
      <style>{HIDE_OVERLAYS_CSS}</style>
      <div
        ref={catcherRef}
        data-testid='screen-color-picker'
        className='fixed inset-0 z-[2147483647]'
        style={{ cursor: 'crosshair', pointerEvents: 'auto' }}
      >
        <div className='pointer-events-none absolute top-3 left-1/2 -translate-x-1/2 rounded-md bg-foreground/85 px-3 py-1.5 text-xs text-background shadow'>
          Click the page to pick a colour · right-click or Esc to cancel
        </div>
      </div>
    </>,
    document.body,
  )
}
