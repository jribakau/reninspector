<script lang="ts">
  import { onDestroy, untrack } from 'svelte'
  import { api, errorText } from '../lib/api'
  import { copyText, labelItems, openContextMenu, type MenuEntry } from '../lib/context.svelte'
  import { previewKind } from '../lib/preview'
  import { storedSize } from '../lib/pane'
  import { layout } from '../lib/settings.svelte'
  import { git } from '../lib/git.svelte'
  import { createEntry, deleteEntry, gameFolder, inGame, moveEntries, renameEntry, revealEntry, toTreePath } from '../lib/fileops.svelte'
  import { openFileHistory } from '../lib/nav.svelte'
  import { app, fileInfo, goTo, nodesInFile, selectLabel, showCode } from '../lib/store.svelte'
  import { baseName, isScriptName, isUnder, joinPath, movedPath, parentOf } from '../lib/treepaths'
  import { ROW } from '../lib/view'
  import { extOf, fileLook } from '../lib/filetypes'
  import Icon from './Icon.svelte'
  import Splitter from './Splitter.svelte'
  import type { DirEntry, MapNode } from '../lib/types'
  import VirtualList from './VirtualList.svelte'

  interface Row {
    entry: DirEntry
    depth: number
    /** The empty row where a new name is typed. */
    isNew?: boolean
    /** An existing row whose name is being edited. */
    renaming?: boolean
  }

  type Edit =
    | { mode: 'new'; dir: boolean; parent: string; value: string; error: string }
    | { mode: 'rename'; dir: boolean; path: string; value: string; error: string }

  interface Drag {
    paths: string[]
    label: string
    icon: ReturnType<typeof fileLook>['icon']
    x: number
    y: number
    /** Folder the drop would land in, '' for the project folder, null when it cannot drop here. */
    over: string | null
  }

  const NEW_KEY = '\u0000new'

  let children = $state<Record<string, DirEntry[]>>({})
  let expanded = $state<Set<string>>(new Set())
  let listErr = $state('')
  let loadedRoot = ''
  const dirGen = new Map<string, number>()
  let outlineEl = $state<HTMLDivElement | null>(null)
  let outlineH = $state(storedSize('vnide.h.outline', 160))
  let treeEl = $state<HTMLDivElement | null>(null)
  let focusIdx = $state(0)
  let pointed = $state<string | null>(null)
  let edit = $state<Edit | null>(null)
  let editSeq = $state(0)
  let committing = false
  let editAt = 0
  let drag = $state<Drag | null>(null)

  let treeOpen = $state(localStorage.getItem('vnide.ex.tree') !== '0')
  let outlineOpen = $state(localStorage.getItem('vnide.ex.outline') !== '0')

  let layoutSeen = layout.seq
  $effect(() => {
    const seq = layout.seq
    if (seq === layoutSeen) return
    layoutSeen = seq
    outlineH = storedSize('vnide.h.outline', 160)
    treeOpen = true
    outlineOpen = true
  })

  function setTreeOpen(open: boolean) {
    treeOpen = open
    localStorage.setItem('vnide.ex.tree', open ? '1' : '0')
  }

  function setOutlineOpen(open: boolean) {
    outlineOpen = open
    localStorage.setItem('vnide.ex.outline', open ? '1' : '0')
  }

  const projectName = $derived(
    (app.info?.root ?? '').replaceAll('\\', '/').replace(/\/+$/, '').split('/').pop() || 'Project',
  )

  function refresh() {
    for (const path of ['', ...expanded]) void loadDir(path)
  }

  /** Folder holding the open file, as a tree path. Lights its indent guide. */
  const activeDir = $derived.by(() => {
    const file = app.loc?.file
    if (!file) return ''
    return parentOf(toTreePath(file))
  })

  async function loadDir(path: string) {
    const root = app.info?.root ?? ''
    const gen = (dirGen.get(path) ?? 0) + 1
    dirGen.set(path, gen)
    try {
      const entries = await api.listDir(path)
      if (dirGen.get(path) !== gen || (app.info?.root ?? '') !== root) return
      children = { ...children, [path]: entries }
      listErr = ''
    } catch (e) {
      if (dirGen.get(path) !== gen) return
      listErr = errorText(e)
    }
  }

  $effect(() => {
    const root = app.info?.root ?? ''
    void app.changeSeq
    if (!root) {
      children = {}
      expanded = new Set()
      dirGen.clear()
      loadedRoot = ''
      listErr = ''
      edit = null
      return
    }
    const switched = root !== loadedRoot
    if (switched) {
      loadedRoot = root
      children = {}
      expanded = new Set()
      dirGen.clear()
      edit = null
    }
    const open = switched ? [''] : ['', ...untrack(() => [...expanded])]
    for (const path of open) void loadDir(path)
  })

  function flatten(path: string, depth: number, out: Row[]) {
    const cur = edit
    if (cur?.mode === 'new' && cur.parent === path) {
      out.push({ entry: { path: NEW_KEY, name: '', dir: cur.dir }, depth, isNew: true })
    }
    for (const entry of children[path] ?? []) {
      out.push({ entry, depth, renaming: cur?.mode === 'rename' && cur.path === entry.path })
      if (entry.dir && expanded.has(entry.path)) flatten(entry.path, depth + 1, out)
    }
  }

  const rows = $derived.by(() => {
    const out: Row[] = []
    flatten('', 0, out)
    return out
  })

  const outline = $derived(app.loc ? nodesInFile(app.loc.file) : [])

  $effect(() => {
    const id = app.selectedLabel
    if (!id || !outlineEl || !outline.some((n) => n.id === id)) return
    outlineEl.querySelector(`[data-id="${CSS.escape(id)}"]`)?.scrollIntoView({ block: 'nearest' })
  })

  function toggleDir(path: string) {
    const next = new Set(expanded)
    if (next.has(path)) next.delete(path)
    else {
      next.add(path)
      if (!children[path]) void loadDir(path)
    }
    expanded = next
  }

  /** Open every folder on the way to `path` (and `path` itself when `self`). */
  function expandTo(path: string, self = true) {
    const parts = path.split('/').filter(Boolean)
    const next = new Set(expanded)
    let acc = ''
    parts.forEach((part, i) => {
      acc = acc ? `${acc}/${part}` : part
      if (i < parts.length - 1 || self) {
        next.add(acc)
        if (!children[acc]) void loadDir(acc)
      }
    })
    expanded = next
  }

  $effect(() => {
    const req = app.explorerReveal
    if (!req) return
    const root = app.info?.root ?? ''
    untrack(() => {
      const parts = toTreePath(req.path).split('/').filter((part) => part.length > 0)
      const parents: string[] = []
      let acc = ''
      for (let i = 0; i < parts.length - 1; i++) {
        acc = acc ? `${acc}/${parts[i]}` : parts[i]
        parents.push(acc)
      }
      void (async () => {
        if ((app.info?.root ?? '') !== root) return
        if (!children['']) await loadDir('')
        for (const parent of parents) {
          if ((app.info?.root ?? '') !== root) return
          if (!children[parent]) await loadDir(parent)
        }
        if ((app.info?.root ?? '') !== root) return
        const next = new Set(expanded)
        let changed = false
        for (const parent of parents) {
          if (!next.has(parent)) {
            next.add(parent)
            changed = true
          }
        }
        if (changed) expanded = next
        setTimeout(() => {
          if (app.explorerReveal?.seq === req.seq) app.explorerReveal = null
        }, 400)
      })()
    })
  })

  const revealIndex = $derived.by(() => {
    if (edit) {
      const at = rows.findIndex((row) => row.isNew || row.renaming)
      if (at >= 0) return at
    }
    const path = app.explorerReveal?.path
    if (!path) return null
    const full = toTreePath(path)
    const index = rows.findIndex((row) => row.entry.path === full)
    return index >= 0 ? index : null
  })

  function openInEditor(path: string, flow: boolean) {
    goTo(path, 1, 1, { flow })
    showCode()
  }

  function openEntry(entry: DirEntry) {
    const rel = inGame(entry.path)
    const ext = extOf(entry.name)
    if (rel && (ext === 'rpy' || ext === 'rpym')) {
      openInEditor(rel, true)
      return
    }
    if (rel && ext === 'rpyc') {
      const rpy = rel.replace(/\.rpyc$/i, '.rpy')
      if (fileInfo(rpy)) {
        openInEditor(rpy, true)
        return
      }
    }
    if (previewKind(entry.name)) openInEditor(entry.path, false)
  }

  /** Why this entry cannot be renamed, moved or deleted, or null when it can. */
  function lockReason(entry: DirEntry): string | null {
    if (entry.path.toLowerCase() === gameFolder().toLowerCase()) return 'The game folder cannot be changed.'
    if (!entry.dir) {
      const rel = inGame(entry.path)
      if (rel && fileInfo(rel)?.origin === 'archived') return 'This file lives in an archive. Save a loose copy first.'
    }
    return null
  }

  /** Where a new file or folder goes when no row was named: the last row clicked, else the open file's folder. */
  function defaultParent(): string {
    if (pointed !== null) {
      const row = rows.find((r) => r.entry.path === pointed)
      if (row && !row.isNew) return row.entry.dir ? row.entry.path : parentOf(row.entry.path)
    }
    return activeDir || gameFolder()
  }

  function startNew(dir: boolean, parent = defaultParent()) {
    if (app.busy) return
    if (!treeOpen) setTreeOpen(true)
    edit = { mode: 'new', dir, parent, value: '', error: '' }
    editAt = performance.now()
    editSeq += 1
    if (parent) expandTo(parent)
  }

  function startRename(entry: DirEntry) {
    if (app.busy) return
    const locked = lockReason(entry)
    if (locked) {
      app.notice = locked
      return
    }
    edit = { mode: 'rename', dir: entry.dir, path: entry.path, value: entry.name, error: '' }
    editAt = performance.now()
    editSeq += 1
  }

  function cancelEdit() {
    if (!committing) edit = null
  }

  async function commitEdit() {
    const cur = edit
    if (!cur || committing) return
    const name = cur.value.trim().replaceAll('\\', '/')
    if (!name) {
      cur.error = 'Type a name.'
      return
    }
    if (cur.mode === 'rename' && name === baseName(cur.path)) {
      edit = null
      return
    }
    committing = true
    try {
      if (cur.mode === 'new') {
        const path = joinPath(cur.parent, name)
        const err = await createEntry(path, cur.dir)
        if (err) {
          cur.error = err
          return
        }
        edit = null
        expandTo(path, cur.dir)
        if (!cur.dir) openEntry({ path, name: baseName(path), dir: false })
      } else {
        const to = joinPath(parentOf(cur.path), name)
        const err = await renameEntry(cur.path, to, cur.dir)
        if (err) {
          cur.error = err
          return
        }
        edit = null
        if (cur.dir) expanded = new Set([...expanded].map((p) => movedPath(p, cur.path, to)))
        pointed = to
      }
    } finally {
      committing = false
    }
  }

  async function remove(entry: DirEntry) {
    const locked = lockReason(entry)
    if (locked) {
      app.notice = locked
      return
    }
    const err = await deleteEntry(entry.path, entry.dir)
    if (err) app.error = err
  }

  function focusEdit(node: HTMLInputElement) {
    node.focus()
    const dot = edit?.mode === 'rename' && !edit.dir ? node.value.lastIndexOf('.') : -1
    node.setSelectionRange(0, dot > 0 ? dot : node.value.length)
  }

  /** A menu closing right after the edit starts hands focus back to the tree; take it again instead of cancelling. */
  function onEditBlur(e: FocusEvent) {
    const input = e.currentTarget as HTMLInputElement
    if (performance.now() - editAt < 250) {
      setTimeout(() => input.isConnected && input.focus(), 0)
      return
    }
    cancelEdit()
  }

  function onEditKey(e: KeyboardEvent) {
    e.stopPropagation()
    if (e.key === 'Enter') {
      e.preventDefault()
      void commitEdit()
    } else if (e.key === 'Escape') {
      e.preventDefault()
      cancelEdit()
    }
  }

  function onTreeKey(e: KeyboardEvent, index: number) {
    if (edit) return
    const row = rows[index]
    if (!row) return
    if (e.key === 'F2') {
      e.preventDefault()
      startRename(row.entry)
    } else if (e.key === 'Delete') {
      e.preventDefault()
      void remove(row.entry)
    } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'c' && !window.getSelection()?.toString()) {
      e.preventDefault()
      void copyText(row.entry.path)
    }
  }

  function absolute(path: string): string {
    const root = (app.info?.root ?? '').replaceAll('\\', '/').replace(/\/+$/, '')
    return root ? `${root}/${path}` : path
  }

  function entryMenu(entry: DirEntry): MenuEntry[] {
    const rel = inGame(entry.path)
    const ext = extOf(entry.name)
    const parent = entry.dir ? entry.path : parentOf(entry.path)
    const locked = lockReason(entry)
    const items: MenuEntry[] = [
      { kind: 'item', label: 'New File…', run: () => startNew(false, parent) },
      { kind: 'item', label: 'New Folder…', run: () => startNew(true, parent) },
      { kind: 'sep' },
    ]
    if (!entry.dir) {
      if (rel && (ext === 'rpy' || ext === 'rpym')) {
        items.push({ kind: 'item', label: 'Open', run: () => openEntry(entry) })
        items.push({ kind: 'item', label: 'Open file history', run: () => openFileHistory(rel) })
      } else if (rel && ext === 'rpyc') {
        const rpy = rel.replace(/\.rpyc$/i, '.rpy')
        items.push({
          kind: 'item',
          label: 'Open script',
          enabled: !!fileInfo(rpy),
          hint: fileInfo(rpy) ? undefined : 'No decompiled script is available.',
          run: () => openEntry(entry),
        })
      } else if (previewKind(entry.name)) {
        items.push({ kind: 'item', label: 'Open', run: () => openEntry(entry) })
      }
      items.push({ kind: 'sep' })
    }
    items.push({ kind: 'item', label: 'Rename…', key: 'F2', enabled: !locked, hint: locked ?? undefined, run: () => startRename(entry) })
    items.push({ kind: 'item', label: 'Delete', key: 'Del', enabled: !locked, hint: locked ?? undefined, run: () => void remove(entry) })
    items.push({ kind: 'sep' })
    items.push({ kind: 'item', label: 'Copy Path', run: () => copyText(absolute(entry.path)) })
    items.push({ kind: 'item', label: 'Copy Relative Path', run: () => copyText(entry.path) })
    const archived = !entry.dir && rel && fileInfo(rel)?.origin === 'archived'
    items.push({
      kind: 'item',
      label: 'Reveal in File Manager',
      enabled: !archived,
      hint: archived ? 'This file lives in an archive.' : undefined,
      run: () => revealEntry(entry.path),
    })
    if (entry.dir) {
      items.push({ kind: 'sep' })
      items.push({ kind: 'item', label: 'Collapse All', run: () => (expanded = new Set()) })
    }
    return items
  }

  function rootMenu(e: MouseEvent) {
    openContextMenu(e, [
      { kind: 'item', label: 'New File…', run: () => startNew(false, '') },
      { kind: 'item', label: 'New Folder…', run: () => startNew(true, '') },
      { kind: 'sep' },
      { kind: 'item', label: 'Reveal in File Manager', run: () => revealEntry('') },
      { kind: 'item', label: 'Refresh', run: refresh },
    ])
  }

  // Drag and drop with pointer events: the window swallows native drag events for OS file drops.
  let dragFrom: { x: number; y: number; entry: DirEntry } | null = null
  let spring: ReturnType<typeof setTimeout> | null = null
  let springFor: string | null = null

  function beginDrag(e: PointerEvent, entry: DirEntry) {
    if (e.button !== 0 || edit || app.busy || lockReason(entry)) return
    pointed = entry.path
    dragFrom = { x: e.clientX, y: e.clientY, entry }
    window.addEventListener('pointermove', onDragMove)
    window.addEventListener('pointerup', onDragEnd)
    window.addEventListener('keydown', onDragKey, true)
  }

  function dropTarget(x: number, y: number, entry: DirEntry): string | null {
    const el = document.elementFromPoint(x, y)
    const node = el?.closest<HTMLElement>('[data-tree-path]')
    let dest: string
    if (node) {
      const path = node.dataset.treePath ?? ''
      dest = node.dataset.treeDir === '1' ? path : parentOf(path)
    } else if (el && treeEl?.contains(el)) {
      dest = ''
    } else {
      return null
    }
    if (parentOf(entry.path) === dest) return null
    if (entry.dir && isUnder(dest, entry.path)) return null
    return dest
  }

  function onDragMove(e: PointerEvent) {
    const from = dragFrom
    if (!from) return
    if (!drag) {
      if (Math.hypot(e.clientX - from.x, e.clientY - from.y) < 5) return
      const look = from.entry.dir ? { icon: 'folder' as const } : fileLook(from.entry.name)
      drag = { paths: [from.entry.path], label: from.entry.name, icon: look.icon, x: e.clientX, y: e.clientY, over: null }
    }
    const over = dropTarget(e.clientX, e.clientY, from.entry)
    drag = { ...drag, x: e.clientX, y: e.clientY, over }
    if (over !== springFor) {
      if (spring) clearTimeout(spring)
      springFor = over
      if (over) {
        spring = setTimeout(() => {
          if (springFor === over && !expanded.has(over)) toggleDir(over)
        }, 600)
      }
    }
  }

  function stopDrag() {
    window.removeEventListener('pointermove', onDragMove)
    window.removeEventListener('pointerup', onDragEnd)
    window.removeEventListener('keydown', onDragKey, true)
    if (spring) clearTimeout(spring)
    spring = null
    springFor = null
    dragFrom = null
    drag = null
  }

  function onDragKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.stopPropagation()
      stopDrag()
    }
  }

  async function onDragEnd() {
    const done = drag
    stopDrag()
    if (!done) return
    // The click that follows a drag must not open the row under the cursor.
    const swallow = (ev: MouseEvent) => ev.stopPropagation()
    window.addEventListener('click', swallow, { capture: true, once: true })
    setTimeout(() => window.removeEventListener('click', swallow, true), 0)
    if (done.over === null) return
    const err = await moveEntries(done.paths, done.over)
    if (err) app.error = err
    else if (done.over) expandTo(done.over)
  }

  onDestroy(stopDrag)

  function labelOf(n: MapNode): string {
    return n.kind === 'screen' ? n.id.slice(7) : n.id
  }
</script>

<div class="sb-panel">
  <div class="sb-head ex-head">
    <h2>Explorer</h2>
  </div>
  {#if listErr}<div class="sb-foot err">{listErr}</div>{/if}
  {#if edit?.error}<div class="sb-foot err" role="alert">{edit.error}</div>{/if}

  <button class="sec" aria-expanded={treeOpen} onclick={() => setTreeOpen(!treeOpen)}>
    <span class="twist" class:open={treeOpen}><Icon name="chevron-right" size={12} /></span>
    <span class="sec-name">{projectName}</span>
  </button>
  {#if treeOpen}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="sb-list virtual tree" class:drop-root={drag?.over === ''} bind:this={treeEl} oncontextmenu={rootMenu}>
      {#if !rows.length && !listErr}
        <div class="empty-state">
          <Icon name="folder" size={28} />
          <p>This folder is empty.</p>
          <button onclick={() => startNew(false, '')}>New file</button>
        </div>
      {:else}
        <VirtualList
          items={rows}
          rowHeight={ROW.tree}
          reveal={revealIndex}
          revealSeq={(app.explorerReveal?.seq ?? 0) + editSeq}
          role="tree"
          bind:active={focusIdx}
          itemKey={(item) => item.entry.path}
          onkey={onTreeKey}
          onhorizontal={(dir, index) => {
            const entry = rows[index]?.entry
            if (!entry?.dir) return
            if ((dir === 'right') !== expanded.has(entry.path)) toggleDir(entry.path)
          }}
        >
          {#snippet row(item)}
            {@const entry = item.entry}
            {#if (item.isNew || item.renaming) && edit}
              {@const look = edit.dir ? null : fileLook(edit.value || 'file')}
              <div class="node editing" style={`--depth:${item.depth}`} role="treeitem" aria-selected="true">
                {@render guides(entry.path, item.depth)}
                <span class="twist" aria-hidden="true">{#if edit.dir}<Icon name="chevron-right" size={12} />{/if}</span>
                <span class={`ficon ${look ? look.tone : 'folder'}`}><Icon name={look ? look.icon : 'folder'} size={15} /></span>
                <input
                  class="inline"
                  class:bad={!!edit.error}
                  bind:value={edit.value}
                  use:focusEdit
                  onkeydown={onEditKey}
                  oninput={() => edit && (edit.error = '')}
                  onblur={onEditBlur}
                  spellcheck="false"
                  autocomplete="off"
                  aria-label={edit.mode === 'new' ? (edit.dir ? 'New folder name' : 'New file name') : 'New name'}
                  aria-invalid={!!edit.error}
                />
              </div>
            {:else if entry.dir}
              {@const open = expanded.has(entry.path)}
              <button
                class="node dir"
                class:sel={activeDir === entry.path}
                class:pointed={pointed === entry.path}
                class:drop={drag?.over === entry.path}
                style={`--depth:${item.depth}`}
                data-tree-path={entry.path}
                data-tree-dir="1"
                role="treeitem"
                aria-level={item.depth + 1}
                aria-expanded={open}
                aria-selected={activeDir === entry.path}
                onpointerdown={(e) => beginDrag(e, entry)}
                onclick={() => {
                  pointed = entry.path
                  toggleDir(entry.path)
                }}
                oncontextmenu={(e) => {
                  pointed = entry.path
                  openContextMenu(e, entryMenu(entry))
                }}
              >
                {@render guides(entry.path, item.depth)}
                <span class="twist" class:open><Icon name="chevron-right" size={12} /></span>
                <span class="ficon folder"><Icon name={open ? 'folder-open' : 'folder'} size={15} /></span>
                <span class="name">{entry.name}</span>
                {#if git.decor.dirs.has(entry.path)}<span class="git-dot" title="This folder has changes"></span>{/if}
              </button>
            {:else}
              {@const rel = inGame(entry.path)}
              {@const info = rel ? fileInfo(rel) : undefined}
              {@const mark = git.decor.files.get(entry.path)}
              {@const look = fileLook(entry.name)}
              {@const isActive = (!!rel && (app.loc?.file === rel || app.explorerReveal?.path === rel)) || app.loc?.file === entry.path}
              <div
                class="node file"
                class:sel={isActive}
                class:pointed={pointed === entry.path}
                style={`--depth:${item.depth}`}
                data-tree-path={entry.path}
                data-tree-dir="0"
                role="treeitem"
                aria-level={item.depth + 1}
                aria-selected={isActive}
                tabindex="-1"
                title={entry.path}
                onpointerdown={(e) => beginDrag(e, entry)}
                onclick={() => {
                  pointed = entry.path
                  openEntry(entry)
                }}
                onkeydown={(e) => { if (e.key === 'Enter') openEntry(entry) }}
                oncontextmenu={(e) => {
                  pointed = entry.path
                  openContextMenu(e, entryMenu(entry))
                }}
              >
                {@render guides(entry.path, item.depth)}
                <span class="twist" aria-hidden="true"></span>
                <span class={`ficon ${look.tone}`}><Icon name={look.icon} size={15} /></span>
                <span class="name" class:git-m={mark === 'M' || mark === 'R'} class:git-a={mark === 'A' || mark === 'U'} class:git-d={mark === 'D'}>{entry.name}</span>
                <span class="marks">
                  {#if info?.origin === 'override'}<span class="mk warn" title={`Loose file hiding the copy in ${info.archive}`}><Icon name="warning" size={12} /></span>{/if}
                  {#if info?.origin === 'archived'}
                    {@const patched = app.info?.archives.find((a) => a.path === info.archive)?.isPatch}
                    <span class="mk" title={patched ? `Patched. Read from ${info.archive}. Saving writes a loose copy.` : `Read from ${info.archive}. Saving writes a loose copy.`}><Icon name="archive" size={12} /></span>
                  {/if}
                  {#if info?.decompiled && !info.editable}<span class="mk warn" title="Decompiled, read-only"><Icon name="lock" size={12} /></span>{/if}
                  {#if rel && app.dirtyFiles.includes(rel)}<span class="mk warn" title="Unsaved changes"><Icon name="dot" size={12} /></span>
                  {:else if rel && app.modifiedFiles.includes(rel)}<span class="mk" title="Modified on disk"><Icon name="dot" size={12} /></span>{/if}
                  {#if mark}<span class="git-letter {mark === 'D' ? 'git-d' : mark === 'A' || mark === 'U' ? 'git-a' : 'git-m'}" title="Git status">{mark}</span>{/if}
                </span>
              </div>
            {/if}
          {/snippet}
        </VirtualList>
      {/if}
    </div>
  {/if}

  {#if treeOpen && outlineOpen}
    <Splitter axis="y" grow={-1} value={outlineH} min={72} hardMax={420} fraction={0.45} reserve={120} reset={160} onchange={(n) => { outlineH = n; localStorage.setItem('vnide.h.outline', String(n)) }} />
  {/if}
  <div class="outline" class:grow={!treeOpen && outlineOpen} style={treeOpen && outlineOpen ? `height:${outlineH}px` : undefined}>
    <button class="sec" aria-expanded={outlineOpen} onclick={() => setOutlineOpen(!outlineOpen)}>
      <span class="twist" class:open={outlineOpen}><Icon name="chevron-right" size={12} /></span>
      <span class="sec-name">Outline{app.loc ? ` · ${app.loc.file.split('/').pop()}` : ''}</span>
      {#if outline.length}<span class="badge">{outline.length}</span>{/if}
    </button>
    {#if outlineOpen}
      <div class="sb-list" bind:this={outlineEl}>
        {#each outline as n (n.id)}
          <button
            class="sb-item"
            data-id={n.id}
            class:sel={app.selectedLabel === n.id}
            onclick={() => selectLabel(n.id)}
            oncontextmenu={(e) => {
              app.selectedLabel = n.id
              openContextMenu(e, labelItems(n.id))
            }}
          >
            <span class="sb-name">
              {labelOf(n)}
              {#if n.kind === 'menu'}<em class="sb-tag menu">menu</em>{/if}
              {#if n.kind === 'screen'}<em class="sb-tag screen">screen</em>{/if}
            </span>
            <span class="sb-meta">line {n.line}</span>
          </button>
        {:else}
          <div class="sb-empty">Open a script to list its labels.</div>
        {/each}
      </div>
    {/if}
  </div>
</div>

{#if drag}
  <div class="ghost" style={`left:${drag.x + 12}px;top:${drag.y + 8}px`} aria-hidden="true">
    <Icon name={drag.icon} size={14} />
    <span>{drag.label}</span>
    {#if drag.over !== null}<em>to {drag.over || projectName}</em>{/if}
  </div>
{/if}

{#snippet guides(path: string, depth: number)}
  {#each { length: depth } as _, g (g)}
    <i class="guide" class:lit={activeDir !== '' && activeDir === path.split('/').slice(0, g + 1).join('/')} style={`--g:${g}`}></i>
  {/each}
{/snippet}

<style>
  .ex-head {
    display: flex;
    flex-flow: row nowrap;
    align-items: center;
    gap: var(--sp-2);
    box-sizing: border-box;
    width: 100%;
    height: var(--h-bar);
    min-height: var(--h-bar);
    max-height: var(--h-bar);
    padding: 0 6px 0 var(--sp-4);
    overflow: hidden;
  }
  .err {
    color: var(--error);
  }
  .sec {
    display: flex;
    align-items: center;
    gap: 4px;
    width: 100%;
    height: 24px;
    flex: none;
    padding: 0 var(--sp-3) 0 6px;
    border: none;
    border-radius: 0;
    border-top: 1px solid var(--line-soft);
    background: transparent;
    color: var(--dim);
    font-size: var(--fs-sm);
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    text-align: left;
  }
  .sec:hover {
    color: var(--text);
    background: var(--hover);
  }
  .sec-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tree {
    flex: 1 1 0;
    user-select: none;
  }
  .tree.drop-root {
    outline: 1px dashed var(--accent);
    outline-offset: -2px;
    background: color-mix(in srgb, var(--accent) 6%, transparent);
  }
  .outline {
    flex: none;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .outline.grow {
    flex: 1 1 0;
  }
  .node {
    position: relative;
    display: flex;
    align-items: center;
    gap: 4px;
    width: 100%;
    height: 100%;
    box-sizing: border-box;
    padding: 0 var(--sp-3) 0 calc(6px + var(--depth) * 14px);
    border: none;
    border-radius: 0;
    background: transparent;
    color: var(--text);
    font-size: var(--fs-md);
    text-align: left;
    cursor: pointer;
  }
  .node:hover {
    background: var(--hover);
  }
  .node.pointed {
    background: var(--hover);
  }
  .node.sel,
  .node.sel:hover {
    background: var(--sel);
  }
  .node.drop,
  .node.drop:hover {
    background: color-mix(in srgb, var(--accent) 24%, transparent);
    outline: 1px dashed var(--accent);
    outline-offset: -1px;
  }
  .node.editing {
    cursor: text;
    background: var(--sel);
  }
  .inline {
    flex: 1;
    min-width: 0;
    height: 20px;
    padding: 0 4px;
    border: 1px solid var(--accent);
    border-radius: var(--r-sm);
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-md);
    outline: none;
    box-shadow: 0 0 0 1px var(--focus-ring);
  }
  .inline.bad {
    border-color: var(--error);
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--error) 50%, transparent);
  }
  .guide {
    position: absolute;
    top: 0;
    bottom: 0;
    left: calc(12px + var(--g) * 14px);
    width: 1px;
    background: var(--line-soft);
    pointer-events: none;
  }
  .guide.lit {
    background: color-mix(in srgb, var(--dim) 55%, transparent);
  }
  .twist {
    width: 14px;
  }
  .ficon {
    display: inline-flex;
    flex: none;
    color: var(--dim);
  }
  .ficon.folder { color: var(--ft-folder); }
  .ficon.script { color: var(--ft-script); }
  .ficon.image { color: var(--ft-image); }
  .ficon.audio { color: var(--ft-audio); }
  .ficon.video { color: var(--ft-video); }
  .ficon.doc { color: var(--ft-doc); }
  .ficon.config { color: var(--ft-config); }
  .ficon.archive { color: var(--ft-archive); }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .name.git-m, .git-letter.git-m { color: var(--warning); }
  .name.git-a, .git-letter.git-a { color: var(--ok); }
  .name.git-d, .git-letter.git-d { color: var(--error); }
  .name.git-d { text-decoration: line-through; }
  .marks {
    display: flex;
    align-items: center;
    gap: 4px;
    flex: none;
  }
  .mk {
    display: inline-flex;
    color: var(--dim);
  }
  .mk.warn {
    color: var(--warning);
  }
  .git-letter {
    min-width: 12px;
    text-align: center;
    font-size: var(--fs-xs);
    font-weight: 700;
  }
  .git-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--warning);
    flex: none;
  }
  .ghost {
    position: fixed;
    z-index: var(--z-context);
    display: flex;
    align-items: center;
    gap: 6px;
    max-width: 280px;
    padding: 3px 8px;
    border: 1px solid var(--line);
    border-radius: var(--r-md);
    background: var(--panel);
    box-shadow: var(--shadow-sm);
    color: var(--text);
    font-size: var(--fs-md);
    pointer-events: none;
    opacity: 0.95;
  }
  .ghost span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ghost em {
    font-style: normal;
    color: var(--dim);
    white-space: nowrap;
  }
</style>
