'use client'

import { BookmarkIcon, XIcon } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { useScene } from '@/hooks/useScene'
import { applyOp } from '@/lib/io/scene'
import { ops } from '@/lib/ops'
import { defaultSizeForPage, useSaveTypesetting, useTypesetting } from '@/lib/typesetting'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'

type Props = {
  /** Font of the selected block (or the panel's current font). */
  selectedFont?: string
  /** Explicit size of the selected block, if it has one. */
  selectedFontSize?: number
  /** Height of the page the selection is on. */
  pageHeight?: number
  hasSelection: boolean
}

/**
 * Project-wide typesetting defaults: tune one block, then save its font and
 * size as the project default. Other pages scale the size by their height.
 * Blocks with their own font/size keep them.
 */
export function ProjectTypesettingDefaults({
  selectedFont,
  selectedFontSize,
  pageHeight,
  hasSelection,
}: Props) {
  const { t } = useTranslation()
  const { scene } = useScene()
  const typesetting = useTypesetting()
  const save = useSaveTypesetting()

  const projectFont = scene?.project?.style?.defaultFont ?? null
  const sizeHere = pageHeight ? defaultSizeForPage(typesetting, pageHeight) : null
  const hasDefaults = !!projectFont || !!typesetting.fontSizePx

  const setProjectFont = (font: string | null) =>
    applyOp(ops.updateProjectMeta({ style: { defaultFont: font } }))

  const saveFromSelection = async () => {
    try {
      if (selectedFont) await setProjectFont(selectedFont)
      if (selectedFontSize && pageHeight) {
        await save.mutateAsync({
          fontSizePx: selectedFontSize,
          referencePageHeight: pageHeight,
        })
      }
    } catch (err) {
      useEditorUiStore.getState().showError(String(err))
    }
  }

  const clear = async () => {
    try {
      await setProjectFont(null)
      await save.mutateAsync({})
    } catch (err) {
      useEditorUiStore.getState().showError(String(err))
    }
  }

  return (
    <div
      className='flex flex-col gap-1.5 rounded-md p-2 ring-1 ring-border'
      data-testid='project-typesetting'
    >
      <span className='text-[10px] font-medium text-muted-foreground uppercase'>
        {t('typesetting.title')}
      </span>
      <div className='grid grid-cols-[auto_minmax(0,1fr)] gap-x-2 gap-y-0.5 text-xs'>
        <span className='text-muted-foreground'>{t('typesetting.font')}</span>
        <span className='truncate' data-testid='project-typesetting-font'>
          {projectFont ?? t('typesetting.notSet')}
        </span>
        <span className='text-muted-foreground'>{t('typesetting.size')}</span>
        <span data-testid='project-typesetting-size'>
          {typesetting.fontSizePx
            ? t('typesetting.sizeValue', {
                size: Math.round(typesetting.fontSizePx),
                ref: typesetting.referencePageHeight ?? '?',
                here: sizeHere ? Math.round(sizeHere) : '?',
              })
            : t('typesetting.notSetAuto')}
        </span>
      </div>
      <div className='flex flex-wrap gap-1'>
        <Button
          variant='outline'
          size='xs'
          className='text-[11px]'
          data-testid='project-typesetting-save'
          disabled={!hasSelection || (!selectedFont && !selectedFontSize) || save.isPending}
          onClick={() => void saveFromSelection()}
        >
          <BookmarkIcon className='size-3' />
          {t('typesetting.saveFromSelection')}
        </Button>
        {hasDefaults && (
          <Button
            variant='ghost'
            size='xs'
            className='text-[11px]'
            data-testid='project-typesetting-clear'
            disabled={save.isPending}
            onClick={() => void clear()}
          >
            <XIcon className='size-3' />
            {t('typesetting.clear')}
          </Button>
        )}
      </div>
      <p className='text-[10px] leading-snug text-muted-foreground'>
        {hasSelection && !selectedFontSize
          ? t('typesetting.hintNoSize')
          : t('typesetting.hint')}
      </p>
    </div>
  )
}
