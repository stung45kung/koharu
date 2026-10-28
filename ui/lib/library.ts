'use client'

import { useMutation, useQuery } from '@tanstack/react-query'

import { fetchApi } from '@/lib/api/fetch'
import type { Character, CharacterBook } from '@/lib/characters'
import type { GlossaryEntry } from '@/lib/glossary'
import { openJsonFile } from '@/lib/io/openFiles'
import { saveBlob } from '@/lib/io/saveBlob'
import { queryClient } from '@/lib/queryClient'

// Hand-written client for `/api/v1/library` (see
// `koharu-rpc/src/routes/library.rs`): characters and glossary terms kept
// one by one, grouped by series, shared by all projects.

export type LibraryCharacter = {
  key: string
  series: string
  updatedAtMs: number
  character: Character
}

export type LibraryTerm = {
  key: string
  series: string
  updatedAtMs: number
  entry: GlossaryEntry
}

export type Library = { characters: LibraryCharacter[]; terms: LibraryTerm[] }

export type MergeOutcome = {
  charactersAdded: number
  charactersUpdated: number
  glossaryAdded: number
  glossaryUpdated: number
}

export const LIBRARY_URL = '/api/v1/library'
export const libraryQueryKey = [LIBRARY_URL] as const

const post = <T>(url: string, body: unknown) =>
  fetchApi<T>(url, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })

export const getLibrary = () => fetchApi<Library>(LIBRARY_URL, { method: 'GET' })

export function useLibrary(enabled: boolean) {
  return useQuery({ queryKey: libraryQueryKey, queryFn: getLibrary, enabled, retry: false })
}

const storeLibrary = (lib: Library) => queryClient.setQueryData(libraryQueryKey, lib)

export function useSaveToLibrary() {
  return useMutation({
    mutationFn: (req: { series: string; characterIds: string[]; glossarySources: string[] }) =>
      post<{ library: Library; added: number; updated: number }>(`${LIBRARY_URL}/save`, req),
    onSuccess: (res) => storeLibrary(res.library),
  })
}

export function useDeleteFromLibrary() {
  return useMutation({
    mutationFn: (req: { characterKeys: string[]; termKeys: string[] }) =>
      post<Library>(`${LIBRARY_URL}/delete`, req),
    onSuccess: storeLibrary,
  })
}

export function useApplyFromLibrary() {
  return useMutation({
    mutationFn: (req: { characterKeys: string[]; termKeys: string[] }) =>
      post<{ outcome: MergeOutcome; book: CharacterBook }>(`${LIBRARY_URL}/apply`, req),
    onSuccess: () =>
      Promise.all([
        queryClient.invalidateQueries({ queryKey: ['/api/v1/characters'] }),
        queryClient.invalidateQueries({ queryKey: ['/api/v1/glossary'] }),
      ]),
  })
}

/** Save the whole library as a JSON file (backup / move to another PC). */
export async function exportLibraryFile(): Promise<boolean> {
  const lib = await getLibrary()
  const blob = new Blob([JSON.stringify(lib, null, 2)], { type: 'application/json' })
  const date = new Date().toISOString().slice(0, 10)
  return saveBlob(blob, `koharu-library-${date}.json`)
}

/**
 * Merge a library JSON file into the library. Returns `null` when the user
 * cancelled the file picker.
 */
export async function importLibraryFile(): Promise<{ added: number; updated: number } | null> {
  const file = await openJsonFile('Koharu library')
  if (!file) return null
  let parsed: unknown
  try {
    parsed = JSON.parse(await file.text())
  } catch {
    throw new Error('not a JSON file')
  }
  const lib = parsed as Partial<Library>
  if (!lib || (!Array.isArray(lib.characters) && !Array.isArray(lib.terms))) {
    throw new Error('not a Koharu library file')
  }
  const res = await post<{ library: Library; added: number; updated: number }>(
    `${LIBRARY_URL}/import`,
    { characters: lib.characters ?? [], terms: lib.terms ?? [] },
  )
  storeLibrary(res.library)
  return { added: res.added, updated: res.updated }
}
