import type { GlossaryEntry } from '@/lib/glossary'

/**
 * Translation checks run in the browser over the whole scene:
 * - `glossary`: the source contains a glossary term but the translation
 *   doesn't contain its required rendering;
 * - `inconsistent`: the same source line is translated differently on
 *   different blocks;
 * - `untranslated`: OCR text but no translation.
 */
export type IssueKind = 'glossary' | 'inconsistent' | 'untranslated'

export type CheckBlock = { id: string; text?: string | null; translation?: string | null }
export type CheckPage = { id: string; blocks: CheckBlock[] }

export type Issue = {
  kind: IssueKind
  pageId: string
  /** 1-based page number in project order. */
  pageNumber: number
  nodeId: string
  /** 1-based block number on its page (same as the Text Blocks panel). */
  blockNumber: number
  source: string
  translation: string
  /** Glossary: the term. Inconsistent: the other renderings. */
  detail: string
  /** Inconsistent: the shared source line (groups the issue). */
  group?: string
}

const flat = (s?: string | null) => (s ?? '').replace(/\s+/g, ' ').trim()
const key = (s: string) => s.replace(/\s+/g, '').toLowerCase()

/** Shortest source line worth comparing (skips "!", "…", single kana). */
const MIN_SOURCE_CHARS = 2

export function checkConsistency(pages: CheckPage[], glossary: GlossaryEntry[]): Issue[] {
  const terms = glossary
    .map((g) => ({ source: g.source.trim(), target: g.target.trim() }))
    .filter((g) => g.source && g.target)
  const issues: Issue[] = []
  const bySource = new Map<string, { issue: Omit<Issue, 'kind' | 'detail'>; tr: string }[]>()

  pages.forEach((page, p) => {
    page.blocks.forEach((block, b) => {
      const source = flat(block.text)
      if (!source) return
      const translation = flat(block.translation)
      const base = {
        pageId: page.id,
        pageNumber: p + 1,
        nodeId: block.id,
        blockNumber: b + 1,
        source,
        translation,
      }
      if (!translation) {
        issues.push({ ...base, kind: 'untranslated', detail: '' })
        return
      }
      const srcKey = key(source)
      const trKey = key(translation)
      for (const term of terms) {
        if (srcKey.includes(key(term.source)) && !trKey.includes(key(term.target))) {
          issues.push({ ...base, kind: 'glossary', detail: `${term.source} → ${term.target}` })
        }
      }
      if ([...srcKey].length >= MIN_SOURCE_CHARS) {
        const list = bySource.get(srcKey) ?? []
        list.push({ issue: base, tr: translation })
        bySource.set(srcKey, list)
      }
    })
  })

  for (const list of bySource.values()) {
    // Renderings that differ only in spacing count as the same.
    const variants = new Map<string, string>()
    for (const { tr } of list) if (!variants.has(key(tr))) variants.set(key(tr), tr)
    if (variants.size < 2) continue
    for (const { issue, tr } of list) {
      issues.push({
        ...issue,
        kind: 'inconsistent',
        group: issue.source,
        detail: [...variants.entries()]
          .filter(([k]) => k !== key(tr))
          .map(([, v]) => v)
          .join(' / '),
      })
    }
  }
  return issues
}
