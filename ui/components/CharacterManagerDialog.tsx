'use client'

import { Loader2Icon, PlusIcon, Trash2Icon } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Textarea } from '@/components/ui/textarea'
import {
  EMPTY_CHARACTER_BOOK,
  newCharacterId,
  useCharactersQuery,
  useSaveCharacters,
  type Character,
} from '@/lib/characters'

type CharacterManagerDialogProps = {
  open: boolean
  onOpenChange: (open: boolean) => void
}

/**
 * Edit the open project's character list (name + free-form speech notes).
 * Changes are kept in a local draft and written on Save; deleting a
 * character that is in use turns its blocks back into "unknown speaker".
 */
export function CharacterManagerDialog({ open, onOpenChange }: CharacterManagerDialogProps) {
  const { t } = useTranslation()
  const query = useCharactersQuery()
  const book = query.data ?? EMPTY_CHARACTER_BOOK
  const save = useSaveCharacters()
  const [draft, setDraft] = useState<Character[]>([])
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
      setDraft(query.data.characters.map((c) => ({ ...c })))
      setError(null)
      setInitialized(true)
    }
  }, [open, initialized, query.isSuccess, query.data])

  const usage = useMemo(() => {
    const counts: Record<string, number> = {}
    for (const id of Object.values(book.speakers)) counts[id] = (counts[id] ?? 0) + 1
    return counts
  }, [book.speakers])

  const update = (id: string, patch: Partial<Character>) =>
    setDraft((prev) => prev.map((c) => (c.id === id ? { ...c, ...patch } : c)))

  const add = () =>
    setDraft((prev) => [...prev, { id: newCharacterId(), name: '', description: '' }])

  const remove = (id: string) => setDraft((prev) => prev.filter((c) => c.id !== id))

  const hasBlankName = draft.some((c) => !c.name.trim())

  const handleSave = async () => {
    if (hasBlankName) {
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
      <DialogContent className='flex max-h-[85vh] max-w-xl flex-col gap-3' data-testid='characters-dialog'>
        <div>
          <DialogTitle>{t('characters.title')}</DialogTitle>
          <DialogDescription className='mt-1 text-xs'>{t('characters.description')}</DialogDescription>
        </div>

        <ScrollArea className='min-h-0 flex-1'>
          <div className='flex flex-col gap-2 pr-2'>
            {draft.length === 0 && (
              <p className='rounded-md border border-dashed border-border p-3 text-xs text-muted-foreground'>
                {t('characters.empty')}
              </p>
            )}
            {draft.map((c, index) => (
              <div
                key={c.id}
                data-testid={`character-row-${index}`}
                className='flex flex-col gap-1.5 rounded-md p-2 ring-1 ring-border'
              >
                <div className='flex items-center gap-1.5'>
                  <Input
                    data-testid={`character-name-${index}`}
                    value={c.name}
                    maxLength={80}
                    placeholder={t('characters.namePlaceholder')}
                    onChange={(e) => update(c.id, { name: e.target.value })}
                    className='h-7 text-xs'
                  />
                  {usage[c.id] ? (
                    <span className='shrink-0 text-[10px] text-muted-foreground'>
                      {t('characters.usedBy', { count: usage[c.id] })}
                    </span>
                  ) : null}
                  <Button
                    data-testid={`character-delete-${index}`}
                    aria-label={t('characters.delete')}
                    variant='ghost'
                    size='icon-xs'
                    onClick={() => remove(c.id)}
                    className='text-rose-600 hover:text-rose-600'
                  >
                    <Trash2Icon className='size-3' />
                  </Button>
                </div>
                <Textarea
                  data-testid={`character-description-${index}`}
                  value={c.description}
                  maxLength={4000}
                  rows={3}
                  placeholder={t('characters.descriptionPlaceholder')}
                  onChange={(e) => update(c.id, { description: e.target.value })}
                  className='min-h-0 resize-y px-2 py-1 text-xs'
                />
              </div>
            ))}
          </div>
        </ScrollArea>

        {error && <p className='text-xs text-rose-600'>{error}</p>}

        <div className='flex items-center justify-between gap-2'>
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
          <div className='flex gap-2'>
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
        </div>
      </DialogContent>
    </Dialog>
  )
}
