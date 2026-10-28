'use client'

import { useMutation, useQuery } from '@tanstack/react-query'

import { useScene } from '@/hooks/useScene'
import { fetchApi } from '@/lib/api/fetch'
import { queryClient } from '@/lib/queryClient'

// Hand-written client for `/api/v1/typesetting`
// (see `koharu-rpc/src/routes/typesetting.rs`).

export type TypesettingDefaults = {
  /** Text size in px, measured on a page `referencePageHeight` px tall. */
  fontSizePx?: number | null
  referencePageHeight?: number | null
  /** RGBA text colour for blocks without their own style. */
  color?: number[] | null
  /** false: keep the size fixed (may overflow). true: shrink long text to fit. */
  shrinkToFit?: boolean
  /** Outline for blocks without their own. */
  stroke?: StrokeDefaults | null
  /** Thai line breaking with ICU's LSTM model instead of its dictionary. */
  thaiLstmLineBreaking?: boolean
  /** true: a block alone in a speech bubble uses the bubble's free area. false: text stays in the block. */
  expandIntoBubble?: boolean
}

/** Default outline; unset colour / width stay automatic. */
export type StrokeDefaults = {
  enabled: boolean
  color?: number[] | null
  /** px on a page `referencePageHeight` tall (scaled like the size). */
  widthPx?: number | null
}

/** Speaker outline over the project's, field by field (mirrors the renderer). */
export function layeredStroke(
  speaker: StrokeDefaults | null | undefined,
  project: StrokeDefaults | null | undefined,
): StrokeDefaults | null {
  if (!speaker && !project) return null
  return {
    enabled: (speaker ?? project)?.enabled ?? true,
    color: speaker?.color ?? project?.color ?? null,
    widthPx: speaker?.widthPx ?? project?.widthPx ?? null,
  }
}

/** Scale a reference-page px value to a page `pageHeight` tall. */
export function scaleForPage(d: TypesettingDefaults, px: number, pageHeight: number): number {
  const ref = d.referencePageHeight
  if (!ref || ref <= 0 || pageHeight <= 0) return px
  return (px * pageHeight) / ref
}

export const TYPESETTING_URL = '/api/v1/typesetting'
export const typesettingQueryKey = [TYPESETTING_URL] as const

export const getTypesetting = () => fetchApi<TypesettingDefaults>(TYPESETTING_URL, { method: 'GET' })

export const putTypesetting = (defaults: TypesettingDefaults) =>
  fetchApi<TypesettingDefaults>(TYPESETTING_URL, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(defaults),
  })

export function useTypesetting(): TypesettingDefaults {
  const { scene } = useScene()
  const projectKey = scene ? `${scene.project?.name ?? ''}|${scene.project?.createdAt ?? ''}` : null
  const { data } = useQuery({
    queryKey: [...typesettingQueryKey, projectKey],
    queryFn: getTypesetting,
    enabled: projectKey !== null,
    retry: false,
    staleTime: Infinity,
  })
  return data ?? {}
}

export function useSaveTypesetting() {
  return useMutation({
    mutationFn: putTypesetting,
    onSuccess: (saved) => {
      queryClient.setQueriesData({ queryKey: typesettingQueryKey, type: 'active' }, saved)
      queryClient.removeQueries({ queryKey: typesettingQueryKey, type: 'inactive' })
    },
  })
}

/** Size the renderer will aim for on a page `pageHeight` px tall. */
export function defaultSizeForPage(d: TypesettingDefaults, pageHeight: number): number | null {
  const size = d.fontSizePx
  if (!size || !Number.isFinite(size) || size <= 0) return null
  const ref = d.referencePageHeight
  if (!ref || ref <= 0 || pageHeight <= 0) return size
  return Math.max(1, (size * pageHeight) / ref)
}

export type ResetBlockStylesResult = { reset: number; epoch?: number | null }

/** Drop the own style of some text blocks (one undo step). */
export const resetBlockStyles = (pageId: string, nodeIds: string[]) =>
  fetchApi<ResetBlockStylesResult>(`${TYPESETTING_URL}/reset-block-styles`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ pageId, nodeIds }),
  })
