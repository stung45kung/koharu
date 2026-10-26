'use client'

import { useMutation, useQuery } from '@tanstack/react-query'

import { useScene } from '@/hooks/useScene'
import { fetchApi } from '@/lib/api/fetch'
import { queryClient } from '@/lib/queryClient'

// Hand-written client for `/api/v1/characters` (see
// `koharu-rpc/src/routes/characters.rs`). Mirrors the Rust `CharacterBook`.

export type Character = {
  id: string
  name: string
  description: string
}

export type CharacterBook = {
  version: number
  characters: Character[]
  /** text node id → character id. Missing key = unknown speaker. */
  speakers: Record<string, string>
  /** Nodes whose speaker was guessed automatically and not yet confirmed. */
  suggested?: string[]
}

export type GuessSpeakersResult = {
  book: CharacterBook
  considered: number
  assigned: number
}

export const EMPTY_CHARACTER_BOOK: CharacterBook = {
  version: 1,
  characters: [],
  speakers: {},
  suggested: [],
}

export const CHARACTERS_URL = '/api/v1/characters'
export const SPEAKERS_URL = '/api/v1/characters/speakers'
export const GUESS_URL = '/api/v1/characters/guess'
export const CONFIRM_URL = '/api/v1/characters/confirm'

export const charactersQueryKey = [CHARACTERS_URL] as const

const jsonInit = (method: string, body: unknown): RequestInit => ({
  method,
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify(body),
})

export const getCharacters = () => fetchApi<CharacterBook>(CHARACTERS_URL, { method: 'GET' })

export const putCharacters = (characters: Character[]) =>
  fetchApi<CharacterBook>(CHARACTERS_URL, jsonInit('PUT', { characters }))

export const putSpeakers = (nodeIds: string[], characterId: string | null) =>
  fetchApi<CharacterBook>(SPEAKERS_URL, jsonInit('PUT', { nodeIds, characterId }))

export const guessSpeakers = (pageId: string, targetLanguage?: string) =>
  fetchApi<GuessSpeakersResult>(GUESS_URL, jsonInit('POST', { pageId, targetLanguage }))

export const confirmSpeakers = (nodeIds: string[]) =>
  fetchApi<CharacterBook>(CONFIRM_URL, jsonInit('POST', { nodeIds }))

/** Query for the open project's character book (disabled with no project). */
export function useCharactersQuery() {
  const { scene } = useScene()
  // Re-fetch when a different project is opened.
  const projectKey = scene ? `${scene.project?.name ?? ''}|${scene.project?.createdAt ?? ''}` : null
  return useQuery({
    queryKey: [...charactersQueryKey, projectKey],
    queryFn: getCharacters,
    enabled: projectKey !== null,
    retry: false,
    staleTime: Infinity,
  })
}

/** Character book of the open project (empty when none / on error). */
export function useCharacters(): CharacterBook {
  return useCharactersQuery().data ?? EMPTY_CHARACTER_BOOK
}

const storeBook = (book: CharacterBook) => {
  // Only the open project's query is active; cached books of other projects
  // are dropped so reopening them refetches instead of showing this one.
  queryClient.setQueriesData({ queryKey: charactersQueryKey, type: 'active' }, book)
  queryClient.removeQueries({ queryKey: charactersQueryKey, type: 'inactive' })
}

export function useSaveCharacters() {
  return useMutation({ mutationFn: putCharacters, onSuccess: storeBook })
}

export function useAssignSpeaker() {
  return useMutation({
    mutationFn: ({ nodeIds, characterId }: { nodeIds: string[]; characterId: string | null }) =>
      putSpeakers(nodeIds, characterId),
    onSuccess: storeBook,
  })
}

export function useGuessSpeakers() {
  return useMutation({
    mutationFn: ({ pageId, targetLanguage }: { pageId: string; targetLanguage?: string }) =>
      guessSpeakers(pageId, targetLanguage),
    onSuccess: (result) => storeBook(result.book),
  })
}

export function useConfirmSpeakers() {
  return useMutation({ mutationFn: confirmSpeakers, onSuccess: storeBook })
}

export function isSuggested(book: CharacterBook, nodeId: string): boolean {
  return book.suggested?.includes(nodeId) ?? false
}

export function speakerOf(book: CharacterBook, nodeId: string): Character | null {
  const id = book.speakers[nodeId]
  if (!id) return null
  return book.characters.find((c) => c.id === id) ?? null
}

/** UUIDv7-ish id for a new character; the backend only needs uniqueness. */
export function newCharacterId(): string {
  if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) return crypto.randomUUID()
  const hex = () => Math.floor(Math.random() * 16).toString(16)
  return 'xxxxxxxx-xxxx-4xxx-8xxx-xxxxxxxxxxxx'.replace(/x/g, hex)
}
