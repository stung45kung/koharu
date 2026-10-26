'use client'

import { Loader2Icon, PlusIcon, Trash2Icon } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import {
  EMPTY_CHARACTER_BOOK,
  newCharacterId,
  useCharactersQuery,
  useSaveCharacters,
  type Character,
} from '@/lib/characters'

const MAX_NAME = 80
const MAX_DESCRIPTION = 4000

type CharacterManagerDialogProps = {
  open: boolean
  onOpenChange: (open: boolean) => void
}

/**
 * Edit the open project's character list (name + free-form speech notes).
 *
 * Two panes: the character list on the left, the selected character's
 * editor on the right (its description box fills the dialog height, so long
 * profiles stay readable however many characters there are).
 *
 * Changes are kept in a local draft and written on Save; deleting a
 * character that is in use turns its blocks back into "unknown speaker".
 */
export function CharacterManagerDialog({ open, onOpenChange }: CharacterManagerDialogProps) {
  const { t } = useTranslation()
  const query = useCharactersQuery()
  const book = query.data ?? EMPTY_CHARACTER_BOOK
  const save = useSaveCharacters()
  const [draft, setDraft] = useState<Character[]>([])
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [initialized, setInitialized] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // Seed the draft once per opening, and only from loaded data: saving a
  // draft seeded before the fetch finished would wipe existing characters.
  useEffect(() => {
    if (!open) {
      setInitialized(false)
      return
    }
    if (!initialized && query.isSuccess) {
      const seeded = query.data.characters.map((c) => ({ ...c }))
      setDraft(seeded)
      setSelectedId(seeded[0]?.id ?? null)
      setError(null)
      setInitialized(true)
    }
  }, [open, initialized, query.isSuccess, query.data])

  const usage = useMemo(() => {
    const counts: Record<string, number> = {}
    for (const id of Object.values(book.speakers)) counts[id] = (counts[id] ?? 0) + 1
    return counts
  }, [book.speakers])

  const selected = draft.find((c) => c.id === selectedId) ?? null

  const update = (id: string, patch: Partial<Character>) =>
    setDraft((prev) => prev.map((c) => (c.id === id ? { ...c, ...patch } : c)))

  const add = () => {
    const id = newCharacterId()
    setDraft((prev) => [...prev, { id, name: '', description: '' }])
    setSelectedId(id)
  }

  const remove = (id: string) => {
    const index = draft.findIndex((c) => c.id === id)
    const next = draft.filter((c) => c.id !== id)
    setDraft(next)
    // Keep a neighbour selected so the editor doesn't go blank.
    setSelectedId(next[Math.min(index, next.length - 1)]?.id ?? null)
  }

  const handleSave = async () => {
    const blank = draft.find((c) => !c.name.trim())
    if (blank) {
      setSelectedId(blank.id)
      setError(t('characters.nameRequired'))
      return
    }
    try {
      await save.mutateAsync(draft)
      onOpenChange(false)
    } catch (err) {
      setError(String(err))
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className='flex h-[85vh] w-[95vw] max-w-5xl flex-col gap-3 sm:max-w-5xl'
        data-testid='characters-dialog'
      >
        <div className='shrink-0 pr-6'>
          <DialogTitle>{t('characters.title')}</DialogTitle>
          <DialogDescription className='mt-1 text-xs'>
            {t('characters.description')}
          </DialogDescription>
        </div>

        <div className='flex min-h-0 flex-1 gap-3'>
          {/* Character list */}
          <div className='flex w-60 shrink-0 flex-col gap-2'>
            <div
              className='min-h-0 flex-1 overflow-y-auto rounded-md p-1 ring-1 ring-border'
              data-testid='character-list'
            >
              {draft.length === 0 && (
                <p className='p-2 text-xs text-muted-foreground'>{t('characters.empty')}</p>
              )}
              {draft.map((c, index) => {
                const active = c.id === selectedId
                return (
                  <button
                    key={c.id}
                    type='button'
                    data-testid={`character-item-${index}`}
                    data-selected={active}
                    onClick={() => setSelectedId(c.id)}
                    className={`flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-xs transition-colors ${
                      active ? 'bg-primary text-primary-foreground' : 'hover:bg-accent'
                    }`}
                  >
                    <span
                      className={`min-w-0 flex-1 truncate ${c.name.trim() ? '' : 'italic opacity-60'}`}
                    >
                      {c.name.trim() || t('characters.unnamed')}
                    </span>
                    {usage[c.id] ? (
                      <span className='shrink-0 text-[10px] opacity-70'>
                        {t('characters.usedBy', { count: usage[c.id] })}
                      </span>
                    ) : null}
                  </button>
                )
              })}
            </div>
            <Button
              variant='outline'
              size='sm'
              onClick={add}
              disabled={!initialized}
              data-testid='character-add'
            >
              <PlusIcon className='size-3.5' />
              {t('characters.add')}
            </Button>
          </div>

          {/* Editor for the selected character */}
          <div className='flex min-w-0 flex-1 flex-col gap-2'>
            {selected ? (
              <>
                <div className='flex shrink-0 items-center gap-2'>
                  <Input
                    data-testid='character-name'
                    value={selected.name}
                    maxLength={MAX_NAME}
                    placeholder={t('characters.namePlaceholder')}
                    onChange={(e) => update(selected.id, { name: e.target.value })}
                    className='h-8 text-sm'
                    autoFocus={!selected.name}
                  />
                  <Button
                    data-testid='character-delete'
                    aria-label={t('characters.delete')}
                    variant='ghost'
                    size='icon-sm'
                    onClick={() => remove(selected.id)}
                    className='text-rose-600 hover:text-rose-600'
                  >
                    <Trash2Icon className='size-4' />
                  </Button>
                </div>
                {usage[selected.id] ? (
                  <p className='shrink-0 text-[10px] text-muted-foreground'>
                    {t('characters.deleteWarning', { count: usage[selected.id] })}
                  </p>
                ) : null}
                <Textarea
                  data-testid='character-description'
                  value={selected.description}
                  maxLength={MAX_DESCRIPTION}
                  placeholder={t('characters.descriptionPlaceholder')}
                  onChange={(e) => update(selected.id, { description: e.target.value })}
                  className='min-h-0 flex-1 resize-none px-3 py-2 text-xs leading-relaxed'
                />
                <p className='shrink-0 text-right text-[10px] text-muted-foreground tabular-nums'>
                  {selected.description.length} / {MAX_DESCRIPTION}
                </p>
              </>
            ) : (
              <div className='flex flex-1 items-center justify-center rounded-md border border-dashed border-border text-xs text-muted-foreground'>
                {t('characters.selectPrompt')}
              </div>
            )}
          </div>
        </div>

        <div className='flex shrink-0 items-center justify-end gap-2'>
          {error && <p className='mr-auto text-xs text-rose-600'>{error}</p>}
          <Button variant='ghost' size='sm' onClick={() => onOpenChange(false)}>
            {t('characters.cancel')}
          </Button>
          <Button
            size='sm'
            onClick={() => void handleSave()}
            disabled={!initialized || save.isPending}
            data-testid='character-save'
          >
            {save.isPending && <Loader2Icon className='size-3.5 animate-spin' />}
            {t('characters.save')}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}
