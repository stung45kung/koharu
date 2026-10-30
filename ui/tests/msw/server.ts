import { http, HttpResponse } from 'msw'
import { setupServer } from 'msw/node'

import { getDefaultMock } from '@/lib/api/default/default.msw'

// Orval-generated handlers seed every endpoint with faker data. Individual
// tests override specific routes via `server.use(http.get(...))`.
//
// `/characters` and `/typesetting` are served by hand-written clients
// (`lib/characters.ts`, `lib/typesetting.ts`), so they get explicit empty
// defaults here.
export const server = setupServer(
  ...getDefaultMock(),
  http.get('/api/v1/characters', () =>
    HttpResponse.json({ version: 1, characters: [], speakers: {} }),
  ),
  http.get('/api/v1/typesetting', () => HttpResponse.json({})),
  http.get('/api/v1/glossary', () => HttpResponse.json({ entries: [] })),
)
