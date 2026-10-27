'use client'

import { useMutation, useQuery } from '@tanstack/react-query'

import { fetchApi } from '@/lib/api/fetch'
import type { CharacterBook } from '@/lib/characters'
import { queryClient } from '@/lib/queryClient'

// Hand-written client for `/api/v1/library` (see
// `koharu-rpc/src/routes/library.rs`): character + glossary presets shared
// between projects.

export type PresetSummary = {
  name: string
  updatedAtMs: number
  characters: number
  glossaryEntries: number
  hasTypesetting: boolean
}

export type MergeOutcome = {
  charactersAdded: number
  charactersUpdated: number
  glossaryAdded: number
  glossaryUpdated: number
  typesettingApplied: boolean
}

export const LIBRARY_URL = '/api/v1/library'
export const libraryQueryKey = [LIBRARY_URL] as const

const post = <T>(url: string, body: unknown) =>
  fetchApi<T>(url, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })

export const listPresets = () => fetchApi<PresetSummary[]>(LIBRARY_URL, { method: 'GET' })
export const savePreset = (name: string) => post<PresetSummary>(`${LIBRARY_URL}/save`, { name })
export const applyPreset = (name: string, applyTypesetting: boolean) =>
  post<{ outcome: MergeOutcome; book: CharacterBook }>(`${LIBRARY_URL}/apply`, {
    name,
    applyTypesetting,
  })
export const deletePreset = (name: string) => post<boolean>(`${LIBRARY_URL}/delete`, { name })

export function useLibrary(enabled: boolean) {
  return useQuery({ queryKey: libraryQueryKey, queryFn: listPresets, enabled, retry: false })
}

const refreshLibrary = () => queryClient.invalidateQueries({ queryKey: libraryQueryKey })

export function useSavePreset() {
  return useMutation({ mutationFn: savePreset, onSuccess: refreshLibrary })
}

export function useDeletePreset() {
  return useMutation({ mutationFn: deletePreset, onSuccess: refreshLibrary })
}

export function useApplyPreset() {
  return useMutation({
    mutationFn: ({ name, applyTypesetting }: { name: string; applyTypesetting: boolean }) =>
      applyPreset(name, applyTypesetting),
    onSuccess: () =>
      Promise.all([
        queryClient.invalidateQueries({ queryKey: ['/api/v1/characters'] }),
        queryClient.invalidateQueries({ queryKey: ['/api/v1/glossary'] }),
        queryClient.invalidateQueries({ queryKey: ['/api/v1/typesetting'] }),
      ]),
  })
}
