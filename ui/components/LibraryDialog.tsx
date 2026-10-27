'use client'

import { ArrowLeftIcon, ArrowRightIcon, Loader2Icon, Trash2Icon } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { useScene } from '@/hooks/useScene'
import { useCharacters } from '@/lib/characters'
import { useGlossaryQuery } from '@/lib/glossary'
import {
  useApplyFromLibrary,
  useDeleteFromLibrary,
  useLibrary,
  useSaveToLibrary,
} from '@/lib/library'
import { cn } from '@/lib/utils'

type Props = {
  open: boolean
  onOpenChange: (open: boolean) => void
}

type Tab = 'characters' | 'terms'

const SERIES_KEY = 'koharu.librarySeries'
const ALL = '\u0000all'

const norm = (s: string) => s.trim().toLowerCase()

const loadSeries = (): string | null => {
  try {
    return localStorage.getItem(SERIES_KEY)
  } catch {
    return null
  }
}

const storeSeries = (s: string) => {
  try {
    localStorage.setItem(SERIES_KEY, s)
  } catch {
    // Only a convenience.
  }
}

function toggle(set: Set<string>, id: string): Set<string> {
  const next = new Set(set)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  return next
}

/**
 * Central library: save chosen characters / glossary terms of this project
 * (grouped by series) and add chosen library items to this project.
 */
export function LibraryDialog({ open, onOpenChange }: Props) {
  const { t } = useTranslation()
  const { scene } = useScene()
  const book = useCharacters()
  const glossary = useGlossaryQuery().data?.entries ?? []
  const library = useLibrary(open)
  const save = useSaveToLibrary()
  const apply = useApplyFromLibrary()
  const remove = useDeleteFromLibrary()

  const [tab, setTab] = useState<Tab>('characters')
  const [series, setSeries] = useState('')
  const [filter, setFilter] = useState(ALL)
  const [search, setSearch] = useState('')
  const [pickProject, setPickProject] = useState<Set<string>>(new Set())
  const [pickLibrary, setPickLibrary] = useState<Set<string>>(new Set())
  const [confirmDelete, setConfirmDelete] = useState(false)
  const [message, setMessage] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!open) return
    setSeries((prev) => prev || loadSeries() || scene?.project?.name || '')
    setMessage(null)
    setError(null)
  }, [open, scene?.project?.name])

  useEffect(() => {
    setPickProject(new Set())
    setPickLibrary(new Set())
    setConfirmDelete(false)
  }, [tab, open])

  const lib = library.data ?? { characters: [], terms: [] }
  const allSeries = useMemo(
    () =>
      [...new Set([...lib.characters, ...lib.terms].map((i) => i.series.trim()))].sort((a, b) =>
        a.localeCompare(b),
      ),
    [lib],
  )

  // Project side.
  const projectItems =
    tab === 'characters'
      ? book.characters.map((c) => ({ id: c.id, label: c.name, sub: '', name: c.name }))
      : glossary.map((g) => ({
          id: g.source,
          label: g.source,
          sub: g.target,
          name: g.source,
        }))
  const inLibrary = (name: string) =>
    tab === 'characters'
      ? lib.characters.some(
          (c) => norm(c.series) === norm(series) && norm(c.character.name) === norm(name),
        )
      : lib.terms.some((x) => norm(x.series) === norm(series) && norm(x.entry.source) === norm(name))

  // Library side.
  const libraryItems = (
    tab === 'characters'
      ? lib.characters.map((c) => ({
          key: c.key,
          series: c.series,
          label: c.character.name,
          sub: '',
          name: c.character.name,
        }))
      : lib.terms.map((x) => ({
          key: x.key,
          series: x.series,
          label: x.entry.source,
          sub: x.entry.target,
          name: x.entry.source,
        }))
  )
    .filter((i) => filter === ALL || norm(i.series) === norm(filter))
    .filter(
      (i) => !search.trim() || `${i.label} ${i.sub}`.toLowerCase().includes(norm(search)),
    )
    .sort((a, b) => a.series.localeCompare(b.series) || a.label.localeCompare(b.label))
  const inProject = (name: string) =>
    tab === 'characters'
      ? book.characters.some((c) => norm(c.name) === norm(name))
      : glossary.some((g) => norm(g.source) === norm(name))

  const run = async (fn: () => Promise<string>) => {
    setError(null)
    setMessage(null)
    try {
      setMessage(await fn())
    } catch (err) {
      setError(String(err))
    }
  }

  const saveSelected = () =>
    void run(async () => {
      storeSeries(series.trim())
      const ids = [...pickProject]
      const res = await save.mutateAsync({
        series: series.trim(),
        characterIds: tab === 'characters' ? ids : [],
        glossarySources: tab === 'terms' ? ids : [],
      })
      setPickProject(new Set())
      return t('library.savedItems', { added: res.added, updated: res.updated })
    })

  const addSelected = () =>
    void run(async () => {
      const keys = [...pickLibrary]
      const res = await apply.mutateAsync({
        characterKeys: tab === 'characters' ? keys : [],
        termKeys: tab === 'terms' ? keys : [],
      })
      setPickLibrary(new Set())
      const o = res.outcome
      return tab === 'characters'
        ? t('library.addedCharacters', { added: o.charactersAdded, updated: o.charactersUpdated })
        : t('library.addedTerms', { added: o.glossaryAdded, updated: o.glossaryUpdated })
    })

  const deleteSelected = () =>
    void run(async () => {
      const keys = [...pickLibrary]
      await remove.mutateAsync({
        characterKeys: tab === 'characters' ? keys : [],
        termKeys: tab === 'terms' ? keys : [],
      })
      setPickLibrary(new Set())
      setConfirmDelete(false)
      return t('library.deletedItems', { count: keys.length })
    })

  const listClass = 'min-h-0 flex-1 overflow-y-auto rounded-md p-1 ring-1 ring-border'
  const rowClass =
    'flex w-full items-center gap-2 rounded px-2 py-1 text-left text-xs hover:bg-accent'

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className='flex h-[85vh] w-[95vw] max-w-5xl flex-col gap-3 sm:max-w-5xl'
        data-testid='library-dialog'
      >
        <div className='shrink-0 pr-6'>
          <DialogTitle>{t('library.title')}</DialogTitle>
          <DialogDescription className='mt-1 text-xs'>{t('library.description')}</DialogDescription>
        </div>

        <div className='flex shrink-0 gap-1'>
          {(['characters', 'terms'] as Tab[]).map((k) => (
            <Button
              key={k}
              size='xs'
              variant={tab === k ? 'default' : 'outline'}
              data-testid={`library-tab-${k}`}
              onClick={() => setTab(k)}
            >
              {t(`library.tab.${k}`)}
            </Button>
          ))}
        </div>

        <div className='grid min-h-0 flex-1 grid-cols-[1fr_auto_1fr] gap-3'>
          {/* This project */}
          <div className='flex min-h-0 flex-col gap-1.5'>
            <span className='text-xs font-medium'>{t('library.thisProject')}</span>
            <div className={listClass} data-testid='library-project-list'>
              {projectItems.length === 0 && (
                <p className='p-2 text-xs text-muted-foreground'>{t('library.projectEmpty')}</p>
              )}
              {projectItems.map((i) => (
                <label key={i.id} className={rowClass}>
                  <input
                    type='checkbox'
                    className='size-3.5 shrink-0 accent-primary'
                    checked={pickProject.has(i.id)}
                    onChange={() => setPickProject((s) => toggle(s, i.id))}
                  />
                  <span className='min-w-0 flex-1 truncate'>
                    {i.label}
                    {i.sub && <span className='text-muted-foreground'> → {i.sub}</span>}
                  </span>
                  {inLibrary(i.name) && (
                    <span className='shrink-0 rounded bg-muted px-1 text-[10px] text-muted-foreground'>
                      {t('library.badgeInLibrary')}
                    </span>
                  )}
                </label>
              ))}
            </div>
            <div className='flex items-center gap-2'>
              <Input
                data-testid='library-series'
                value={series}
                maxLength={80}
                list='library-series-options'
                placeholder={t('library.seriesPlaceholder')}
                onChange={(e) => setSeries(e.target.value)}
                className='h-7 text-xs'
              />
              <datalist id='library-series-options'>
                {allSeries.map((s) => (
                  <option key={s} value={s} />
                ))}
              </datalist>
            </div>
          </div>

          {/* Actions */}
          <div className='flex flex-col items-center justify-center gap-2'>
            <Button
              size='xs'
              data-testid='library-save-selected'
              disabled={pickProject.size === 0 || save.isPending}
              onClick={saveSelected}
              title={t('library.saveSelected')}
            >
              {save.isPending ? (
                <Loader2Icon className='size-3 animate-spin' />
              ) : (
                <ArrowRightIcon className='size-3' />
              )}
              {pickProject.size || ''}
            </Button>
            <Button
              size='xs'
              variant='outline'
              data-testid='library-add-selected'
              disabled={pickLibrary.size === 0 || apply.isPending || !scene}
              onClick={addSelected}
              title={t('library.addSelected')}
            >
              {apply.isPending ? (
                <Loader2Icon className='size-3 animate-spin' />
              ) : (
                <ArrowLeftIcon className='size-3' />
              )}
              {pickLibrary.size || ''}
            </Button>
          </div>

          {/* Library */}
          <div className='flex min-h-0 flex-col gap-1.5'>
            <span className='text-xs font-medium'>{t('library.library')}</span>
            <div className='flex items-center gap-2'>
              <select
                data-testid='library-filter'
                value={filter}
                onChange={(e) => setFilter(e.target.value)}
                className='h-7 rounded-md border border-input bg-transparent px-1 text-xs'
              >
                <option value={ALL}>{t('library.allSeries')}</option>
                {allSeries.map((s) => (
                  <option key={s} value={s}>
                    {s || t('library.noSeries')}
                  </option>
                ))}
              </select>
              <Input
                value={search}
                placeholder={t('library.search')}
                onChange={(e) => setSearch(e.target.value)}
                className='h-7 text-xs'
              />
            </div>
            <div className={listClass} data-testid='library-list'>
              {library.isLoading && (
                <p className='p-2 text-xs text-muted-foreground'>{t('library.loading')}</p>
              )}
              {!library.isLoading && libraryItems.length === 0 && (
                <p className='p-2 text-xs text-muted-foreground'>{t('library.empty')}</p>
              )}
              {libraryItems.map((i, idx) => (
                <div key={i.key}>
                  {(idx === 0 || libraryItems[idx - 1].series !== i.series) && (
                    <div className='px-2 pt-1.5 pb-0.5 text-[10px] font-semibold text-muted-foreground uppercase'>
                      {i.series || t('library.noSeries')}
                    </div>
                  )}
                  <label className={rowClass}>
                    <input
                      type='checkbox'
                      className='size-3.5 shrink-0 accent-primary'
                      checked={pickLibrary.has(i.key)}
                      onChange={() => setPickLibrary((s) => toggle(s, i.key))}
                    />
                    <span className='min-w-0 flex-1 truncate'>
                      {i.label}
                      {i.sub && <span className='text-muted-foreground'> → {i.sub}</span>}
                    </span>
                    {inProject(i.name) && (
                      <span className='shrink-0 rounded bg-muted px-1 text-[10px] text-muted-foreground'>
                        {t('library.badgeInProject')}
                      </span>
                    )}
                  </label>
                </div>
              ))}
            </div>
            <div className='flex items-center justify-end gap-2'>
              {confirmDelete ? (
                <Button
                  size='xs'
                  variant='destructive'
                  data-testid='library-delete-confirm'
                  disabled={remove.isPending}
                  onClick={deleteSelected}
                >
                  {t('library.confirmDelete', { count: pickLibrary.size })}
                </Button>
              ) : (
                <Button
                  size='xs'
                  variant='ghost'
                  className={cn('text-rose-600 hover:text-rose-600')}
                  data-testid='library-delete'
                  disabled={pickLibrary.size === 0}
                  onClick={() => setConfirmDelete(true)}
                >
                  <Trash2Icon className='size-3' />
                  {t('library.delete')}
                </Button>
              )}
            </div>
          </div>
        </div>

        <div className='flex shrink-0 items-center justify-end gap-2'>
          <p className='mr-auto text-[10px] text-muted-foreground'>{t('library.hint')}</p>
          {message && <p className='text-xs text-emerald-600'>{message}</p>}
          {error && <p className='text-xs text-rose-600'>{error}</p>}
          <Button variant='ghost' size='sm' onClick={() => onOpenChange(false)}>
            {t('library.close')}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}
