'use client'

import { BookmarkIcon, ChevronDownIcon, ChevronRightIcon, XIcon } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
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
  /** Effective RGBA colour of the selected block. */
  selectedColor?: number[]
  hasSelection: boolean
}

const toHex = (c: number[]) =>
  '#' +
  c
    .slice(0, 3)
    .map((v) => Math.max(0, Math.min(255, Math.round(v))).toString(16).padStart(2, '0'))
    .join('')

const COLLAPSED_KEY = 'koharu.projectDefaults.collapsed'

function readCollapsed(): boolean {
  try {
    return window.localStorage.getItem(COLLAPSED_KEY) === '1'
  } catch {
    return false
  }
}

function writeCollapsed(value: boolean) {
  try {
    window.localStorage.setItem(COLLAPSED_KEY, value ? '1' : '0')
  } catch {
    // Storage unavailable: the section just won't remember its state.
  }
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
  selectedColor,
  hasSelection,
}: Props) {
  const { t } = useTranslation()
  const { scene } = useScene()
  const typesetting = useTypesetting()
  const save = useSaveTypesetting()
  const [collapsed, setCollapsed] = useState(readCollapsed)
  const toggleCollapsed = () => {
    setCollapsed((prev) => {
      writeCollapsed(!prev)
      return !prev
    })
  }

  const projectFont = scene?.project?.style?.defaultFont ?? null
  const sizeHere = pageHeight ? defaultSizeForPage(typesetting, pageHeight) : null
  const projectColor = typesetting.color ?? null
  const hasDefaults = !!projectFont || !!typesetting.fontSizePx || !!projectColor

  const setProjectFont = (font: string | null) =>
    applyOp(ops.updateProjectMeta({ style: { defaultFont: font } }))

  const saveFromSelection = async () => {
    try {
      if (selectedFont) await setProjectFont(selectedFont)
      const sizePart =
        selectedFontSize && pageHeight
          ? { fontSizePx: selectedFontSize, referencePageHeight: pageHeight }
          : {
              fontSizePx: typesetting.fontSizePx ?? null,
              referencePageHeight: typesetting.referencePageHeight ?? null,
            }
      await save.mutateAsync({
        ...sizePart,
        color: selectedColor ?? typesetting.color ?? null,
        shrinkToFit: typesetting.shrinkToFit ?? false,
      })
    } catch (err) {
      useEditorUiStore.getState().showError(String(err))
    }
  }

  const setShrink = async (shrinkToFit: boolean) => {
    try {
      await save.mutateAsync({ ...typesetting, shrinkToFit })
    } catch (err) {
      useEditorUiStore.getState().showError(String(err))
    }
  }

  const setThaiLstm = async (thaiLstmLineBreaking: boolean) => {
    try {
      await save.mutateAsync({ ...typesetting, thaiLstmLineBreaking })
    } catch (err) {
      useEditorUiStore.getState().showError(String(err))
    }
  }

  const clear = async () => {
    try {
      await setProjectFont(null)
      // Keep the line-breaking choice: it isn't one of the cleared defaults.
      await save.mutateAsync({ thaiLstmLineBreaking: typesetting.thaiLstmLineBreaking })
    } catch (err) {
      useEditorUiStore.getState().showError(String(err))
    }
  }

  return (
    <div
      className='flex flex-col gap-1.5 rounded-md p-2 ring-1 ring-border'
      data-testid='project-typesetting'
    >
      <button
        type='button'
        data-testid='project-typesetting-toggle'
        aria-expanded={!collapsed}
        onClick={toggleCollapsed}
        className='flex w-full items-center gap-1 text-left text-[10px] font-medium text-muted-foreground uppercase hover:text-foreground'
      >
        {collapsed ? (
          <ChevronRightIcon className='size-3' />
        ) : (
          <ChevronDownIcon className='size-3' />
        )}
        <span>{t('typesetting.title')}</span>
        {collapsed && hasDefaults && (
          <span className='ml-auto truncate font-normal normal-case'>
            {[
              projectFont,
              typesetting.fontSizePx ? `${Math.round(typesetting.fontSizePx)}px` : null,
              projectColor ? toHex(projectColor) : null,
            ]
              .filter(Boolean)
              .join(' · ')}
          </span>
        )}
      </button>
      {!collapsed && (
        <>
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
        <span className='text-muted-foreground'>{t('typesetting.color')}</span>
        <span className='flex items-center gap-1.5' data-testid='project-typesetting-color'>
          {projectColor ? (
            <>
              <span
                className='inline-block size-3 rounded-sm ring-1 ring-border'
                style={{ backgroundColor: toHex(projectColor) }}
              />
              {toHex(projectColor)}
            </>
          ) : (
            t('typesetting.notSetColor')
          )}
        </span>
      </div>
      {typesetting.fontSizePx ? (
        <label className='flex items-center gap-2 text-xs'>
          <Switch
            size='sm'
            data-testid='project-typesetting-shrink'
            checked={typesetting.shrinkToFit ?? false}
            disabled={save.isPending}
            onCheckedChange={(checked) => void setShrink(checked)}
          />
          <span>{t('typesetting.shrinkToFit')}</span>
        </label>
      ) : null}
      <label
        className='flex items-center gap-2 text-xs'
        title={t('typesetting.thaiLstmHint')}
      >
        <Switch
          size='sm'
          data-testid='project-typesetting-thai-lstm'
          checked={typesetting.thaiLstmLineBreaking ?? false}
          disabled={save.isPending}
          onCheckedChange={(checked) => void setThaiLstm(checked)}
        />
        <span>{t('typesetting.thaiLstm')}</span>
      </label>
      <div className='flex flex-wrap gap-1'>
        <Button
          variant='outline'
          size='xs'
          className='text-[11px]'
          data-testid='project-typesetting-save'
          disabled={!hasSelection || save.isPending}
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
        </>
      )}
    </div>
  )
}
