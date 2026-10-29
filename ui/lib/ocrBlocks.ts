'use client'

import { getConfig, startPipeline } from '@/lib/api/default/default'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'

/**
 * Run only the configured OCR engine on some text blocks of a page, e.g.
 * after moving or resizing a block that was read wrong. Other blocks keep
 * their text.
 */
export async function ocrBlocks(pageId: string, nodeIds: string[]) {
  if (nodeIds.length === 0) return
  const cfg = await getConfig()
  const ocr = cfg.pipeline?.ocr
  if (!ocr) return
  await startPipeline({
    steps: [ocr],
    pages: [pageId],
    textNodeIds: nodeIds,
    targetLanguage: useEditorUiStore.getState().selectedLanguage,
  })
}
