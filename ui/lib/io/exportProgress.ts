'use client'

import { create } from 'zustand'

/** Progress of a page-by-page export to a folder (null when none runs). */
type ExportProgressState = {
  progress: { done: number; total: number } | null
  cancelled: boolean
  start: (total: number) => void
  step: () => void
  finish: () => void
  cancel: () => void
}

export const useExportProgressStore = create<ExportProgressState>()((set) => ({
  progress: null,
  cancelled: false,
  start: (total) => set({ progress: { done: 0, total }, cancelled: false }),
  step: () =>
    set((s) => (s.progress ? { progress: { ...s.progress, done: s.progress.done + 1 } } : s)),
  finish: () => set({ progress: null, cancelled: false }),
  cancel: () => set({ cancelled: true }),
}))
