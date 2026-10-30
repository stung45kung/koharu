'use client'

import { useQuery } from '@tanstack/react-query'

import { useScene } from '@/hooks/useScene'
import { fetchApi } from '@/lib/api/fetch'
import { startPipeline } from '@/lib/api/default/default'
import { queryClient } from '@/lib/queryClient'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'

// Hand-written client for `/api/v1/ocr-fixes` (see
// `koharu-rpc/src/routes/ocr_fixes.rs`): the OCR text the LLM OCR check
// replaced, per text block, kept outside the scene.

/** Engine id of the LLM OCR check step (koharu-app `ocr-check`). */
export const OCR_CHECK_STEP = 'ocr-check'

export type OcrFixes = {
  /** text node id → OCR text before the LLM check changed it. */
  originals: Record<string, string>
}

export const OCR_FIXES_URL = '/api/v1/ocr-fixes'
export const ocrFixesQueryKey = [OCR_FIXES_URL] as const

const EMPTY: OcrFixes = { originals: {} }

export const getOcrFixes = () => fetchApi<OcrFixes>(OCR_FIXES_URL, { method: 'GET' })

export const forgetOcrFixes = async (nodeIds: string[]) => {
  const fixes = await fetchApi<OcrFixes>(`${OCR_FIXES_URL}/forget`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ nodeIds }),
  })
  queryClient.setQueriesData({ queryKey: ocrFixesQueryKey, type: 'active' }, fixes)
  return fixes
}

/** OCR fixes of the open project (empty when none / on error). */
export function useOcrFixes(): OcrFixes {
  const { scene } = useScene()
  // Re-fetch when a different project is opened.
  const projectKey = scene ? `${scene.project?.name ?? ''}|${scene.project?.createdAt ?? ''}` : null
  const query = useQuery({
    queryKey: [...ocrFixesQueryKey, projectKey],
    queryFn: getOcrFixes,
    enabled: projectKey !== null,
    retry: false,
    staleTime: Infinity,
  })
  return query.data ?? EMPTY
}

/**
 * The OCR text a block had before the LLM check fixed it, or `null` when
 * the block was not fixed (or its text was since set back to it).
 */
export function fixedFrom(fixes: OcrFixes, nodeId: string, text: string | null | undefined) {
  const original = fixes.originals[nodeId]
  return original !== undefined && original !== (text ?? '') ? original : null
}

/** Check some blocks' OCR text with the loaded vision LLM. */
export async function checkOcrBlocks(pageId: string, nodeIds: string[]) {
  if (nodeIds.length === 0) return
  await startPipeline({
    steps: [OCR_CHECK_STEP],
    pages: [pageId],
    textNodeIds: nodeIds,
    targetLanguage: useEditorUiStore.getState().selectedLanguage,
  })
}
