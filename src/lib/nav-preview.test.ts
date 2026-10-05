import { beforeEach, describe, expect, it } from 'vitest'
import { setDirty } from './edit.svelte'
import { setInfo, setMap } from './indexes.svelte'
import { applyLive, resumeFollow } from './live.svelte'
import { app, editorTabId, emptyLive } from './model.svelte'
import {
  activateEditor,
  canReopenClosed,
  closeEditor,
  goTo,
  navBack,
  navForward,
  openLabelGraph,
  resetEditorMemory,
} from './nav.svelte'
import { setSetting, settings } from './settings.svelte'
import type { FileInfo, LiveState, MapNode, ProjectInfo } from './types'

function file(path: string): FileInfo {
  return {
    path,
    lines: 20,
    bytes: 1,
    opaque: 0,
    labels: 1,
    issues: 0,
    origin: 'loose',
    archive: null,
    editable: true,
    decompiled: false,
    reasons: [],
  }
}

function label(id: string, fileIndex: number, line = 1): MapNode {
  return {
    id,
    kind: 'label',
    file: fileIndex,
    line,
    endLine: line + 4,
    stmts: 1,
    says: 1,
    menus: 0,
    choices: 0,
    inDegree: 0,
    outDegree: 0,
    reachable: true,
    root: id === 'start',
    indirect: false,
    duplicate: false,
    returns: false,
    endsScript: false,
    dynamicOut: 0,
  }
}

function live(filePath: string, line: number): LiveState {
  const base = emptyLive()
  return {
    running: true,
    file: filePath,
    line,
    label: base.label,
    showing: base.showing,
    speaker: base.speaker,
    vars: base.vars,
    canWarp: base.canWarp,
    canReload: base.canReload,
    replay: base.replay,
    replayReason: base.replayReason,
    note: base.note,
    warpNotes: base.warpNotes,
  }
}

beforeEach(() => {
  const files = ['a.rpy', 'b.rpy', 'c.rpy', 'd.rpy', 'e.rpy'].map(file)
  setInfo({ root: 'game', files } as ProjectInfo)
  setMap({
    nodes: [label('start', 0), label('next', 1), label('other', 2, 2)],
    edges: [],
  })
  resetEditorMemory()
  app.editorTabs = []
  app.pinnedTabs = []
  app.activeEditor = null
  app.loc = null
  app.cursor = null
  app.selectedLabel = null
  app.dirtyFiles = []
  app.live = emptyLive()
  app.notice = ''
  app.noticeAction = null
  setSetting('maxTabs', 16)
  setSetting('previewTabs', true)
  setSetting('followGame', true)
})

describe('preview tabs', () => {
  it('reuses one tab when peeking through many scripts', () => {
    for (const path of ['a.rpy', 'b.rpy', 'c.rpy', 'd.rpy', 'e.rpy']) goTo(path, 1)
    expect(app.editorTabs).toEqual([{ kind: 'file', path: 'e.rpy' }])
    expect(app.previewTab).toBe('file:e.rpy')
  })

  it('keeps a tab that was opened on purpose', () => {
    goTo('a.rpy', 1, 1, { open: true })
    goTo('b.rpy', 1)
    expect(app.editorTabs.map(editorTabId)).toEqual(['file:a.rpy', 'file:b.rpy'])
    expect(app.previewTab).toBe('file:b.rpy')
  })

  it('promotes a preview when the file is edited', () => {
    goTo('a.rpy', 1)
    setDirty('a.rpy', true)
    expect(app.previewTab).toBeNull()
    goTo('b.rpy', 1)
    expect(app.editorTabs.map(editorTabId)).toEqual(['file:a.rpy', 'file:b.rpy'])
    expect(app.previewTab).toBe('file:b.rpy')
  })

  it('opens a flow tab without a script tab, and the next label replaces it', () => {
    openLabelGraph('start')
    expect(app.editorTabs).toEqual([{ kind: 'graph', name: 'start' }])
    expect(app.loc).toBeNull()
    expect(app.previewTab).toBe('graph:start')
    openLabelGraph('next')
    expect(app.editorTabs).toEqual([{ kind: 'graph', name: 'next' }])
    expect(app.previewTab).toBe('graph:next')
  })
})

describe('history', () => {
  it('keeps replaced previews out of the reopen history', () => {
    for (const path of ['a.rpy', 'b.rpy', 'c.rpy', 'd.rpy']) goTo(path, 1)
    expect(canReopenClosed()).toBe(false)
  })

  it('goes back after a flow preview replaced the open script', () => {
    goTo('a.rpy', 1, 1, { open: true })
    goTo('b.rpy', 1)
    openLabelGraph('start')
    expect(app.loc).toBeNull()
    navBack()
    expect(app.loc?.file).toBe('a.rpy')
    navForward()
    expect(app.loc?.file).toBe('b.rpy')
  })

  it('does not swap out the active preview for a background jump', () => {
    goTo('a.rpy', 1)
    goTo('b.rpy', 3, 3, { activate: false })
    expect(app.editorTabs.map(editorTabId)).toEqual(['file:a.rpy', 'file:b.rpy'])
    expect(app.activeEditor).toBe('file:a.rpy')
    expect(app.previewTab).toBe('file:a.rpy')
  })
})

describe('live follow', () => {
  it('catches up when follow is resumed, and when a save ends the edit', () => {
    goTo('a.rpy', 1, 1, { open: true })
    app.live = { ...emptyLive(), ...live('b.rpy', 2), receivedAt: 1 }
    app.followHold = true
    resumeFollow()
    expect(app.followHold).toBe(false)
    expect(app.loc?.file).toBe('b.rpy')

    goTo('a.rpy', 1, 1, { open: true })
    setDirty('a.rpy', true)
    app.followHold = true
    setDirty('a.rpy', false)
    expect(app.followHold).toBe(false)
  })

  it('does not record a back spot or close a tab that was opened on purpose', () => {
    goTo('a.rpy', 1, 1, { open: true })
    goTo('b.rpy', 1, 1, { open: true })
    applyLive(live('c.rpy', 4))
    expect(app.editorTabs.map(editorTabId)).toEqual(['file:a.rpy', 'file:b.rpy', 'file:c.rpy'])
    expect(app.previewTab).toBe('file:c.rpy')
    expect(app.loc?.file).toBe('c.rpy')
    navBack()
    expect(app.loc?.file).toBe('a.rpy')
  })

  it('stays put when every tab was opened on purpose and the strip is full', () => {
    settings.maxTabs = 2
    goTo('a.rpy', 1, 1, { open: true })
    goTo('b.rpy', 1, 1, { open: true })
    applyLive(live('c.rpy', 4))
    expect(app.editorTabs.map(editorTabId)).toEqual(['file:a.rpy', 'file:b.rpy'])
    expect(app.loc?.file).toBe('b.rpy')
  })

  it('does not move the editor while a flow tab is active', () => {
    goTo('a.rpy', 1, 1, { open: true })
    openLabelGraph('start', { open: true })
    applyLive(live('c.rpy', 2))
    expect(app.activeEditor).toBe('graph:start')
    expect(app.loc?.file).toBe('a.rpy')
    expect(app.editorTabs.map(editorTabId)).toEqual(['file:a.rpy', 'graph:start'])
    expect(app.selectedLabel).toBe('other')
  })

  it('pauses while the active file is dirty or the user is typing', () => {
    goTo('a.rpy', 1, 1, { open: true })
    setDirty('a.rpy', true)
    applyLive(live('b.rpy', 1))
    expect(app.loc?.file).toBe('a.rpy')
    setDirty('a.rpy', false)
    app.followHold = true
    applyLive(live('b.rpy', 1))
    expect(app.loc?.file).toBe('a.rpy')
    goTo('a.rpy', 3, 3, { open: true })
    expect(app.followHold).toBe(false)
  })
})

describe('closing and the tab limit', () => {
  it('lands on the tab used most recently', async () => {
    goTo('a.rpy', 1, 1, { open: true })
    goTo('b.rpy', 1, 1, { open: true })
    goTo('c.rpy', 1, 1, { open: true })
    activateEditor('file:a.rpy')
    await closeEditor('file:a.rpy')
    expect(app.activeEditor).toBe('file:c.rpy')
  })

  it('closes the least recently used tab and offers to reopen it', () => {
    settings.maxTabs = 2
    goTo('a.rpy', 1, 1, { open: true })
    goTo('b.rpy', 1, 1, { open: true })
    goTo('c.rpy', 1, 1, { open: true })
    expect(app.editorTabs.map(editorTabId)).toEqual(['file:b.rpy', 'file:c.rpy'])
    expect(app.notice).toContain('a.rpy')
    expect(app.noticeAction?.label).toBe('Reopen')
  })
})
