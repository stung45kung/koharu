import { http, HttpResponse } from 'msw'
import { setupServer } from 'msw/node'

import { getDefaultMock } from '@/lib/api/default/default.msw'

// Orval-generated handlers seed every endpoint with faker data. Individual
// tests override specific routes via `server.use(http.get(...))`.
//
// `/characters` is served by a hand-written client (`lib/characters.ts`), so
// it gets an explicit empty default here.
export const server = setupServer(
  ...getDefaultMock(),
  http.get('/api/v1/characters', () =>
    HttpResponse.json({ version: 1, characters: [], speakers: {} }),
  ),
)
