'use client'

import { create } from 'zustand'

// In-app colour picking from the page canvas. The browser EyeDropper API
// is not available in the desktop app's WebView, so the "Pick" button of
// colour pickers asks for the colour under the next click on the page.

type ColorPickState = {
  /** Receives the picked colour; `null` when not picking. */
  onPick: ((hex: string) => void) | null
  start: (onPick: (hex: string) => void) => void
  finish: (hex: string) => void
  cancel: () => void
}

export const useColorPickStore = create<ColorPickState>()((set, get) => ({
  onPick: null,
  start: (onPick) => set({ onPick }),
  finish: (hex) => {
    const onPick = get().onPick
    set({ onPick: null })
    onPick?.(hex)
  },
  cancel: () => set({ onPick: null }),
}))

const toHex = (r: number, g: number, b: number) =>
  `#${[r, g, b].map((v) => v.toString(16).padStart(2, '0')).join('')}`.toUpperCase()

const isShown = (el: Element) => {
  for (let node: Element | null = el; node; node = node.parentElement) {
    const style = getComputedStyle(node)
    if (style.display === 'none' || style.visibility === 'hidden') return false
    if (Number.parseFloat(style.opacity) === 0) return false
  }
  return true
}

/**
 * Colour of the topmost visible page layer (rendered page, text sprites,
 * inpainted or source image) under the screen point, or `null` when the
 * point is not over the page.
 */
export function sampleCanvasColor(clientX: number, clientY: number): string | null {
  const workspace = document.querySelector('[data-testid="workspace-canvas"]')
  if (!workspace) return null
  const probe = document.createElement('canvas')
  probe.width = 1
  probe.height = 1
  const ctx = probe.getContext('2d', { willReadFrequently: true })
  if (!ctx) return null

  // Later images sit above earlier ones (the rendered page is last).
  const images = Array.from(workspace.querySelectorAll('img')).reverse()
  for (const img of images) {
    if (!img.complete || !img.naturalWidth || !img.naturalHeight) continue
    const rect = img.getBoundingClientRect()
    if (
      clientX < rect.left ||
      clientX >= rect.right ||
      clientY < rect.top ||
      clientY >= rect.bottom ||
      !isShown(img)
    ) {
      continue
    }
    const sx = Math.min(
      img.naturalWidth - 1,
      Math.floor(((clientX - rect.left) / rect.width) * img.naturalWidth),
    )
    const sy = Math.min(
      img.naturalHeight - 1,
      Math.floor(((clientY - rect.top) / rect.height) * img.naturalHeight),
    )
    ctx.clearRect(0, 0, 1, 1)
    ctx.drawImage(img, sx, sy, 1, 1, 0, 0, 1, 1)
    const [r, g, b, a] = ctx.getImageData(0, 0, 1, 1).data
    // Transparent pixel of a text sprite: look at the layer below.
    if (a < 128) continue
    return toHex(r, g, b)
  }
  return null
}
