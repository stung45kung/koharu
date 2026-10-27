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
