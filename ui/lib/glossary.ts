'use client'

import { useMutation, useQuery } from '@tanstack/react-query'

import { useScene } from '@/hooks/useScene'
import { fetchApi } from '@/lib/api/fetch'
import { queryClient } from '@/lib/queryClient'

// Hand-written client for `/api/v1/glossary` (koharu-rpc `routes/glossary.rs`).

export type GlossaryEntry = { source: string; target: string; note: string }
export type Glossary = { entries: GlossaryEntry[] }

export const GLOSSARY_URL = '/api/v1/glossary'
export const glossaryQueryKey = [GLOSSARY_URL] as const

export const getGlossary = () => fetchApi<Glossary>(GLOSSARY_URL, { method: 'GET' })

export const putGlossary = (glossary: Glossary) =>
  fetchApi<Glossary>(GLOSSARY_URL, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(glossary),
  })

export function useGlossaryQuery() {
  const { scene } = useScene()
  const projectKey = scene ? `${scene.project?.name ?? ''}|${scene.project?.createdAt ?? ''}` : null
  return useQuery({
    queryKey: [...glossaryQueryKey, projectKey],
    queryFn: getGlossary,
    enabled: projectKey !== null,
    retry: false,
    staleTime: Infinity,
  })
}

export function useSaveGlossary() {
  return useMutation({
    mutationFn: putGlossary,
    onSuccess: (saved) => {
      queryClient.setQueriesData({ queryKey: glossaryQueryKey, type: 'active' }, saved)
      queryClient.removeQueries({ queryKey: glossaryQueryKey, type: 'inactive' })
    },
  })
}

export type SuggestTermsResult = {
  suggestions: GlossaryEntry[]
  linesScanned: number
  truncated: boolean
}

/** Ask the loaded LLM for names / terms worth adding (nothing is saved). */
export const suggestTerms = (targetLanguage?: string) =>
  fetchApi<SuggestTermsResult>(`${GLOSSARY_URL}/suggest`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ targetLanguage }),
  })
