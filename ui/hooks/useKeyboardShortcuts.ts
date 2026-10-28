'use client'

import { useEffect, useMemo } from 'react'

import { useHotkeys } from 'react-hotkeys-hook'

import { assignSpeakerToNodes, cachedCharacterBook, OTHER_SPEAKER_ID } from '@/lib/characters'
import { deleteTextNodes, redoOp, selectAllTextNodesOnCurrentPage, undoOp } from '@/lib/io/scene'
import { getPlatform, formatShortcut, isModifierKey } from '@/lib/shortcutUtils'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import { useSelectionStore } from '@/lib/stores/selectionStore'

export function useKeyboardShortcuts() {
  const setMode = useEditorUiStore((state) => state.setMode)
  const setBrushConfig = usePreferencesStore((state) => state.setBrushConfig)
  const shortcuts = usePreferencesStore((state) => state.shortcuts)
  const isMac = useMemo(() => getPlatform() === 'mac', [])
  const pageId = useSelectionStore((s) => s.pageId)
  const selectedIds = useSelectionStore((s) => s.nodeIds)

  // Optimized tool mapping - built once and updated only when shortcuts change
  const TOOL_MAP = useMemo(
    (): Record<string, import('@/lib/types').ToolMode> => ({
      [shortcuts.select]: 'select',
      [shortcuts.block]: 'block',
      [shortcuts.brush]: 'brush',
      [shortcuts.eraser]: 'eraser',
      [shortcuts.repairBrush]: 'repairBrush',
    }),
    [shortcuts],
  )

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      const target = event.target
      const inTextField =
        target instanceof HTMLElement &&
        (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable)

      // Undo / Redo — these work globally, including from within text fields,
      // as scene-level history should usually take precedence over native
      // browser text-undo.
      const shortcut = formatShortcut(event, isMac)
      const mod = isMac ? event.metaKey : event.ctrlKey

      if (shortcut === shortcuts.undo) {
        event.preventDefault()
        void undoOp()
        return
      }

      if (shortcut === shortcuts.redo) {
        event.preventDefault()
        void redoOp()
        return
      }

      // Legacy fallback: Redo on Ctrl+Y / Cmd+Y
      if (mod && (event.key === 'y' || event.key === 'Y')) {
        event.preventDefault()
        void redoOp()
        return
      }

      // Select all text blocks on the current page. Runs outside text fields;
      // inside a textarea/input the browser's native "select all text" wins.
      if (mod && (event.key === 'a' || event.key === 'A') && !inTextField) {
        event.preventDefault()
        selectAllTextNodesOnCurrentPage()
        return
      }

      // Every other shortcut is tool-level and should not fire while typing.
      if (inTextField) return

      // Early exit for modifier-only events
      if (isModifierKey(event.key)) {
        return
      }

      // Tool Switching - O(1) direct matching
      const matchingTool = shortcut ? TOOL_MAP[shortcut] : undefined
      if (matchingTool) {
        setMode(matchingTool)
        return
      }

      // Speaker of the selected blocks: 1-9 = character in list order,
      // 0 = unknown, O = other (not in the list).
      const plain = !event.ctrlKey && !event.metaKey && !event.altKey
      const selected = useSelectionStore.getState().nodeIds
      if (plain && selected.size > 0) {
        let speaker: string | null | undefined
        if (/^[1-9]$/.test(event.key)) {
          speaker = cachedCharacterBook().characters[Number(event.key) - 1]?.id
        } else if (event.key === '0') {
          speaker = null
        } else if (event.key === 'o' || event.key === 'O') {
          speaker = OTHER_SPEAKER_ID
        }
        if (speaker !== undefined) {
          event.preventDefault()
          void assignSpeakerToNodes([...selected], speaker).catch((err) =>
            useEditorUiStore.getState().showError(String(err)),
          )
          return
        }
      }

      // Brush Size
      if (shortcut === shortcuts.increaseBrushSize) {
        const currentSize = usePreferencesStore.getState().brushConfig.size
        setBrushConfig({ size: Math.min(128, currentSize + 4) })
      } else if (shortcut === shortcuts.decreaseBrushSize) {
        const currentSize = usePreferencesStore.getState().brushConfig.size
        setBrushConfig({ size: Math.max(8, currentSize - 4) })
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isMac, setMode, TOOL_MAP, shortcuts])

  // Delete removes the selected text blocks (form fields keep their own
  // Delete: react-hotkeys-hook ignores inputs and textareas).
  useHotkeys(
    'delete',
    (event) => {
      // Delete in the page list deletes pages, not text blocks.
      const target = event.target as Element | null
      if (target?.closest?.('[data-testid="navigator-panel"]')) return
      if (pageId && selectedIds.size > 0) void deleteTextNodes(pageId, selectedIds)
    },
    { enabled: !!pageId && selectedIds.size > 0 },
    [pageId, selectedIds],
  )
}
