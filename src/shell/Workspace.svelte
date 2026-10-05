<script lang="ts">
  import ActivityBar from '../components/ActivityBar.svelte'
  import Breadcrumbs from '../components/Breadcrumbs.svelte'
  import Icon from '../components/Icon.svelte'
  import type { IconName } from '../lib/icons'
  import Splitter from '../components/Splitter.svelte'
  import BottomPanel from '../components/BottomPanel.svelte'
  import CodeView from '../components/CodeView.svelte'
  import DiffView from '../components/DiffView.svelte'
  import RebaseView from '../components/RebaseView.svelte'
  import FilePreview from '../components/FilePreview.svelte'
  import LabelGraph from '../components/LabelGraph.svelte'
  import StagePane from '../components/StagePane.svelte'
  import LiveStrip from '../components/LiveStrip.svelte'
  import ProjectMap from '../components/ProjectMap.svelte'
  import Sidebar from '../components/Sidebar.svelte'
  import { onDestroy, untrack } from 'svelte'
  import { openContextMenu, openMenuBelow, tabBarItems, tabItems, tabListItems } from '../lib/context.svelte'
  import { tabTitle } from '../lib/tabs'
  import { bindStage } from '../lib/stage.svelte'
  import { storedSize } from '../lib/pane'
  import { layout, settings } from '../lib/settings.svelte'
  import { previewKind } from '../lib/preview'
  import {
    activateEditor,
    app,
    closeEditor,
    cursorMoved,
    diagnosticsFor,
    editorTabId,
    fileInfo,
    fileOfNode,
    followFlowLabel,
    goTo,
    labelAt,
    nodeByName,
    nodesInFile,
    noteStale,
    openActivity,
    openLabelGraph,
    openMapTab,
    promoteTab,
    renameSymbol,
    isPinned,
    moveEditor,
    unpinEditor,
    revertFile,
    saveFile,
    revealInExplorer,
    selectLabel,
    setDirty,
    showCode,
    showReferences,
    toggleFlow,
    toggleStage,
    toggleScene,
    type EditorTab,
  } from '../lib/store.svelte'

  bindStage()

  const W_SIDE = 'vnide.w.side'
  const W_FLOW = 'vnide.w.flow'
  const H_BOTTOM = 'vnide.h.bottom'
  const H_STAGE = 'vnide.h.stage'
  let sideW = $state(storedSize(W_SIDE, 280))
  let flowW = $state(storedSize(W_FLOW, 420))
  let bottomH = $state(storedSize(H_BOTTOM, 200))
  let stageH = $state(storedSize(H_STAGE, 240))
  let layoutSeen = layout.seq
  $effect(() => {
    const seq = layout.seq
    if (seq === layoutSeen) return
    layoutSeen = seq
    sideW = storedSize(W_SIDE, 280)
    flowW = storedSize(W_FLOW, 420)
    bottomH = storedSize(H_BOTTOM, 200)
    stageH = storedSize(H_STAGE, 240)
  })
  let tabsEl: HTMLElement | undefined = $state()
  let fadeLeft = $state(false)
  let fadeRight = $state(false)

  function setSide(n: number) {
    sideW = n
    localStorage.setItem(W_SIDE, String(n))
  }
  function setFlow(n: number) {
    flowW = n
    localStorage.setItem(W_FLOW, String(n))
  }
  function setBottom(n: number) {
    bottomH = n
    localStorage.setItem(H_BOTTOM, String(n))
  }
  function setStage(n: number) {
    stageH = n
    localStorage.setItem(H_STAGE, String(n))
  }

  function showCenterCode() {
    showCode()
  }
  function showCenterFlow() {
    if (!app.sceneExpanded) toggleScene()
  }

  function tabScroll() {
    const node = tabsEl
    if (!node) return
    fadeLeft = node.scrollLeft > 4
    fadeRight = node.scrollLeft + node.clientWidth < node.scrollWidth - 4
  }

  /** A vertical wheel over the strip scrolls the tabs sideways. */
  function horizontalWheel(node: HTMLElement) {
    const onWheel = (e: WheelEvent) => {
      if (node.scrollWidth <= node.clientWidth + 1) return
      const delta = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY
      if (!delta) return
      e.preventDefault()
      node.scrollLeft += delta
    }
    node.addEventListener('wheel', onWheel, { passive: false })
    return {
      destroy() {
        node.removeEventListener('wheel', onWheel)
      },
    }
  }

  $effect(() => {
    const on = settings.previewTabs
    const preview = app.previewTab
    // Only these two are tracked. Saving the session reads far more state.
    if (!on && preview) untrack(() => promoteTab(preview))
  })

  $effect(() => {
    void app.activeEditor
    void app.editorTabs.length
    queueMicrotask(() => {
      tabsEl?.querySelector('.tab.on')?.scrollIntoView({ inline: 'nearest', block: 'nearest' })
      tabScroll()
    })
  })

  // Tab reordering uses pointer events: the window swallows native drag events for OS file drops.
  interface TabDrag {
    id: string
    x: number
    y: number
    /** Where the tab would land: before the tab now at this index. Null when released here would cancel. */
    slot: number | null
    /** Pixel offset of the drop marker inside the tab strip. */
    mark: number
  }

  let drag = $state<TabDrag | null>(null)
  let dragFrom: { x: number; y: number; id: string } | null = null
  let scrollDir = 0
  let scroller: ReturnType<typeof setInterval> | null = null

  function beginTabDrag(e: PointerEvent, id: string) {
    if (e.button !== 0 || (e.target instanceof Element && e.target.closest('.x'))) return
    dragFrom = { x: e.clientX, y: e.clientY, id }
    window.addEventListener('pointermove', onTabMove)
    window.addEventListener('pointerup', onTabDrop)
    window.addEventListener('keydown', onTabDragKey, true)
  }

  function tabSlot(x: number, y: number): { slot: number | null; mark: number } {
    const strip = tabsEl
    if (!strip) return { slot: null, mark: 0 }
    const bar = strip.parentElement?.getBoundingClientRect()
    if (bar && (y < bar.top - 24 || y > bar.bottom + 24)) return { slot: null, mark: 0 }
    const rects = [...strip.querySelectorAll<HTMLElement>('.tab')].map((el) => el.getBoundingClientRect())
    const origin = strip.getBoundingClientRect().left - strip.scrollLeft
    let slot = rects.length
    for (let i = 0; i < rects.length; i++) {
      if (x < rects[i].left + rects[i].width / 2) {
        slot = i
        break
      }
    }
    const edge = slot < rects.length ? rects[slot].left : (rects[rects.length - 1]?.right ?? origin)
    return { slot, mark: edge - origin }
  }

  function onTabMove(e: PointerEvent) {
    const from = dragFrom
    if (!from) return
    if (!drag && Math.hypot(e.clientX - from.x, e.clientY - from.y) < 5) return
    drag = { id: from.id, x: e.clientX, y: e.clientY, ...tabSlot(e.clientX, e.clientY) }
    const box = tabsEl?.getBoundingClientRect()
    scrollDir = !box ? 0 : e.clientX < box.left + 32 ? -1 : e.clientX > box.right - 32 ? 1 : 0
    if (scrollDir && !scroller) {
      scroller = setInterval(() => {
        if (!scrollDir || !drag || !tabsEl) return
        tabsEl.scrollLeft += scrollDir * 14
        drag = { ...drag, ...tabSlot(drag.x, drag.y) }
      }, 16)
    }
  }

  function stopTabDrag() {
    window.removeEventListener('pointermove', onTabMove)
    window.removeEventListener('pointerup', onTabDrop)
    window.removeEventListener('keydown', onTabDragKey, true)
    if (scroller) clearInterval(scroller)
    scroller = null
    scrollDir = 0
    dragFrom = null
    drag = null
  }

  function onTabDragKey(e: KeyboardEvent) {
    if (e.key !== 'Escape') return
    e.stopPropagation()
    stopTabDrag()
  }

  function onTabDrop() {
    const from = dragFrom
    const done = drag
    stopTabDrag()
    if (!from) return
    const index = app.editorTabs.findIndex((t) => editorTabId(t) === from.id)
    // Dropping back on the same tab is a click. The drag used to swallow that click,
    // so the small pinned icon looked like it did nothing.
    const sameSlot = !!done && (done.slot === index || done.slot === index + 1)
    if (!done || sameSlot) {
      activateEditor(from.id)
      return
    }
    if (done.slot === null) return
    const swallow = (ev: MouseEvent) => ev.stopPropagation()
    window.addEventListener('click', swallow, { capture: true, once: true })
    setTimeout(() => window.removeEventListener('click', swallow, true), 0)
    moveEditor(from.id, done.slot)
  }

  onDestroy(stopTabDrag)

  /** Alt+Left and Alt+Right move the focused tab one place. */
  function onTabKey(e: KeyboardEvent, id: string) {
    if (!e.altKey || (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight')) return
    const index = app.editorTabs.findIndex((t) => editorTabId(t) === id)
    if (index < 0) return
    e.preventDefault()
    const step = e.key === 'ArrowLeft' ? -1 : 1
    const neighbour = app.editorTabs[index + step]
    // Pinning is a deliberate act, so the keys stop at the edge of the pinned block.
    if (!neighbour || isPinned(editorTabId(neighbour)) !== isPinned(id)) return
    moveEditor(id, step < 0 ? index - 1 : index + 2)
  }

  let listBtn: HTMLButtonElement | undefined = $state()
  const showTabList = $derived(app.editorTabs.length > 0 && (fadeLeft || fadeRight || app.editorTabs.length > 6))

  const activeTab = $derived(app.editorTabs.find((t) => editorTabId(t) === app.activeEditor) ?? null)
  const activeKind = $derived(activeTab?.kind ?? 'file')
  const openedPreview = $derived(app.loc && !fileInfo(app.loc.file) ? previewKind(app.loc.file) : null)
  const mediaKind = $derived(openedPreview === 'image' || openedPreview === 'audio' || openedPreview === 'video' ? openedPreview : null)
  const sceneMain = $derived(app.sceneExpanded && activeKind === 'file' && app.editorTabs.length > 0)
  const fileCenter = $derived(activeTab?.kind === 'file')
  const centerFlow = $derived(sceneMain)

  $effect(() => {
    if (!sceneMain) return
    const el = document.activeElement
    if (el instanceof HTMLElement && el.closest('.pane.hidden')) el.blur()
  })

  const graphName = $derived(activeTab?.kind === 'graph' ? activeTab.name : null)
  const diffTab = $derived(activeTab?.kind === 'diff' ? activeTab : null)
  const rebaseTab = $derived(activeTab?.kind === 'rebase' ? activeTab : null)
  const graphNode = $derived(graphName ? nodeByName(graphName) : undefined)
  const graphFile = $derived(graphNode ? fileOfNode(graphNode) : null)

  const flowNode = $derived.by(() => {
    if (!app.selectedLabel) return undefined
    const node = nodeByName(app.selectedLabel)
    if (!node || node.kind === 'missing' || node.kind === 'screen' || node.kind === 'compiled') return undefined
    return node
  })
  const flowFile = $derived(flowNode ? fileOfNode(flowNode) : null)
  const flowEmpty = $derived.by(() => {
    const file = app.loc?.file
    if (!file) return 'Open a script to see its flow.'
    const has = nodesInFile(file).some((n) => n.kind === 'label' || n.kind === 'menu')
    return has ? 'Move the caret into a label to see its flow.' : 'This script has no label to draw.'
  })

  const caretLabel = $derived.by(() => {
    if (!app.loc || !app.cursor || app.cursor.file !== app.loc.file) return null
    return labelAt(app.loc.file, app.cursor.line)
  })

  const openDiags = $derived.by(() => {
    void app.diag
    return diagnosticsFor(app.loc?.file)
  })

  function originHint(path: string | undefined): string {
    if (!path || !app.info) return ''
    const file = fileInfo(path)
    if (!file) return ''
    if (file.origin === 'archived') {
      return `inside ${file.archive}; saving writes a loose copy the game uses instead`
    }
    if (file.origin === 'override') {
      return `overrides ${file.archive}; package edits to fold it into a patch`
    }
    if (file.decompiled) {
      const compiled = file.path.replace(/\.rpym$/, '.rpymc').replace(/\.rpy$/, '.rpyc')
      if (!file.editable) {
        const why = file.reasons.length ? ` (${file.reasons.slice(0, 4).join('; ')})` : ''
        return `Decompiled from ${compiled}, but not completely${why}. Read-only, so a save cannot drop code.`
      }
      return `Decompiled from ${compiled}. Comments and formatting are not recoverable.`
    }
    return ''
  }

  function fileEditable(path: string | undefined): boolean {
    if (!path || !app.info) return true
    const file = fileInfo(path)
    return file?.editable !== false
  }

  function tabIcon(t: EditorTab): IconName {
    if (t.kind === 'map') return 'map'
    if (t.kind === 'graph') return 'flow'
    if (t.kind === 'diff' || t.kind === 'rebase') return 'diff'
    const kind = previewKind(t.path)
    if (kind === 'image') return 'image'
    if (kind === 'audio') return 'audio'
    if (kind === 'video') return 'video'
    return 'file'
  }

  function tabLive(t: EditorTab): boolean {
    if (!app.live.running) return false
    if (t.kind === 'graph') return app.live.label === t.name
    if (t.kind === 'file') return app.live.file === t.path
    return false
  }

  function cursorLine(file: string | null): number | null {
    return app.cursor && file && app.cursor.file === file ? app.cursor.line : null
  }

  function liveLine(file: string | null): number | null {
    return app.live.running && file && app.live.file === file ? app.live.line : null
  }
</script>

<div class="workspace">
  <div class="body">
    <ActivityBar />
    <aside class:closed={!app.sidebarOpen} style={app.sidebarOpen ? `width:${sideW}px` : ''}>
      <Sidebar />
    </aside>
    {#if app.sidebarOpen}
      <Splitter axis="x" grow={1} value={sideW} min={200} hardMax={600} fraction={0.35} reserve={280} reset={280} onchange={setSide} />
    {/if}

    <section class="editor">
      <div class="tabbar" class:fade-left={fadeLeft} class:fade-right={fadeRight}>
        <div class="tabs" bind:this={tabsEl} use:horizontalWheel onscroll={tabScroll} role="tablist" aria-label="Editors">
          {#each app.editorTabs as t (editorTabId(t))}
            {@const id = editorTabId(t)}
            {@const dirty = t.kind === 'file' && app.dirtyFiles.includes(t.path)}
            {@const pinned = isPinned(id)}
            <div
              class="tab"
              role="presentation"
              class:on={app.activeEditor === id}
              class:live={tabLive(t)}
              class:pinned
              class:preview={app.previewTab === id}
              class:dragging={drag?.id === id}
              onpointerdown={(e) => beginTabDrag(e, id)}
              oncontextmenu={(e) => openContextMenu(e, tabItems(t))}
              onmousedown={(e) => { if (e.button === 1) e.preventDefault() }}
              onauxclick={(e) => { if (e.button === 1) { e.preventDefault(); void closeEditor(id) } }}
            >
              <button
                class="tl"
                role="tab"
                aria-selected={app.activeEditor === id}
                onclick={() => activateEditor(id)}
                ondblclick={() => promoteTab(id)}
                onkeydown={(e) => onTabKey(e, id)}
                title={pinned
                  ? `Pinned: ${tabTitle(t)}`
                  : app.previewTab === id
                    ? `Preview: ${tabTitle(t)}. Double-click to keep it open.`
                    : t.kind === 'file'
                      ? `${t.path}${dirty ? ' (unsaved)' : ''}`
                      : tabTitle(t)}
              >
                <span class="ti"><Icon name={tabIcon(t)} size={14} /></span>
                {#if !pinned}<span class="tt">{tabTitle(t)}</span>{/if}
                {#if tabLive(t)}<i class="livedot" title="Live" aria-label="Live"></i>{/if}
              </button>
              <button
                class="x"
                class:dirty
                onclick={() => void (pinned ? unpinEditor(id) : closeEditor(id))}
                aria-label={pinned ? `Unpin ${tabTitle(t)}` : `Close ${tabTitle(t)}`}
                title={pinned ? 'Unpin tab' : undefined}
              >
                <i class="dot"></i>
                <span class="glyph"><Icon name={pinned ? 'pin' : 'close'} size={12} /></span>
              </button>
            </div>
          {/each}
          {#if drag && drag.slot !== null}
            <i class="drop-mark" style={`left:${drag.mark}px`} aria-hidden="true"></i>
          {/if}
        </div>
        <span
          class="spacer"
          role="presentation"
          oncontextmenu={(e) => {
            if (!app.editorTabs.length) {
              e.preventDefault()
              return
            }
            openContextMenu(e, tabBarItems())
          }}
        ></span>
        {#if showTabList}
          <button
            class="icon ghost tablist"
            bind:this={listBtn}
            onclick={() => listBtn && openMenuBelow(listBtn, tabListItems())}
            aria-label="Show open editors"
            aria-haspopup="menu"
            title="Open editors"
          >
            <Icon name="chevron-down" size={14} />
          </button>
        {/if}
        {#if fileCenter}
          <div class="modes">
            <div class="seg" role="radiogroup" aria-label="Center view">
              <button
                class="seg-btn"
                role="radio"
                aria-checked={!centerFlow}
                class:on={!centerFlow}
                onclick={showCenterCode}
                title="Show the script (Ctrl+E)"
              >
                <Icon name="code" size={14} /> Code
              </button>
              <button
                class="seg-btn"
                role="radio"
                aria-checked={centerFlow}
                class:on={centerFlow}
                onclick={showCenterFlow}
                title="Fill the window with this label's flow (Ctrl+E)"
              >
                <Icon name="flow" size={14} /> Flow
              </button>
            </div>
            {#if !centerFlow}
              <div class="side" role="group" aria-label="Side panes">
                <button
                  class="icon ghost"
                  class:on={app.stageOpen}
                  aria-pressed={app.stageOpen}
                  aria-label="Stage preview"
                  onclick={toggleStage}
                  title="Stage preview beside the script (Ctrl+Shift+V)"
                >
                  <Icon name="stage" size={15} />
                </button>
                <button
                  class="icon ghost"
                  class:on={app.flowOpen}
                  aria-pressed={app.flowOpen}
                  aria-label="Flow beside the script"
                  onclick={toggleFlow}
                  title="Flow graph beside the script (Ctrl+Shift+L)"
                >
                  <Icon name="split" size={15} />
                </button>
              </div>
            {/if}
          </div>
        {/if}
      </div>
      {#if app.live.running}
        <LiveStrip />
      {/if}
      <div class="editor-row">
        <div class="stage">
          {#if !app.editorTabs.length}
            <div class="pane empty-workspace empty-state">
              <h2>{app.info?.name ?? 'No file open'}</h2>
              <p>Open a script, or jump somewhere in the project.</p>
              <div class="empty-actions">
                <button class="primary" onclick={() => (app.palette = 'files')}>Go to file</button>
                <button onclick={openMapTab}>Project map</button>
                {#if app.recentFiles.length}
                  <button onclick={() => goTo(app.recentFiles[0], 1)}>Open recent</button>
                {:else}
                  <button onclick={() => openActivity('explorer')}>Show explorer</button>
                {/if}
              </div>
              <ul class="shortcuts" aria-label="Keyboard shortcuts">
                <li><span class="kbd">Ctrl</span><span class="kbd">P</span> Go to file</li>
                <li><span class="kbd">Ctrl</span><span class="kbd">Shift</span><span class="kbd">P</span> Commands</li>
                <li><span class="kbd">Ctrl</span><span class="kbd">T</span> Go to symbol</li>
              </ul>
            </div>
          {/if}
          <div class="pane" class:hidden={activeKind !== 'file' || sceneMain || !!mediaKind || !app.editorTabs.length}>
            {#if app.loc}
              <Breadcrumbs
                path={app.loc.file}
                label={caretLabel ? (caretLabel.kind === 'screen' ? caretLabel.id.slice(7) : caretLabel.id) : null}
                onsegment={revealInExplorer}
                onlabel={() => caretLabel && selectLabel(caretLabel.id)}
              />
            {/if}
            <CodeView
              loc={mediaKind ? null : app.loc}
              diagnostics={openDiags}
              changeSeq={app.changeSeq}
              changedPaths={app.changedPaths}
              editing={fileEditable(app.loc?.file)}
              dirty={!!app.loc && app.dirtyFiles.includes(app.loc.file)}
              modified={!!app.loc && app.modifiedFiles.includes(app.loc.file)}
              reloadFile={app.reloadFile}
              liveLine={liveLine(app.loc?.file ?? null)}
              oncursor={cursorMoved}
              ondirty={setDirty}
              onsave={saveFile}
              onrevert={revertFile}
              onstale={noteStale}
              ongoto={goTo}
              onrefs={showReferences}
              onrename={renameSymbol}
              hint={originHint(app.loc?.file)}
            />
          </div>
          {#if mediaKind && app.loc && activeKind === 'file' && !sceneMain}
            <div class="pane">
              <Breadcrumbs path={app.loc.file} onsegment={revealInExplorer} />
              {#key app.loc.file}
                <FilePreview path={app.loc.file} kind={mediaKind} />
              {/key}
            </div>
          {/if}
          {#if graphName}
            <div class="pane">
              {#key graphName}
                <LabelGraph
                  name={graphName}
                  file={graphFile}
                  cursorLine={cursorLine(graphFile)}
                  liveLine={liveLine(graphFile)}
                  reloadKey={app.changeSeq}
                  onopen={followFlowLabel}
                  ongoto={goTo}
                />
              {/key}
            </div>
          {/if}
          {#if sceneMain}
            <div class="pane">
              {#if flowNode}
                {#key flowNode.id}
                  <LabelGraph
                    name={flowNode.id}
                    file={flowFile}
                    cursorLine={cursorLine(flowFile)}
                    liveLine={liveLine(flowFile)}
                    reloadKey={app.changeSeq}
                    onopen={followFlowLabel}
                    ongoto={goTo}
                  />
                {/key}
              {:else}
                <div class="flow-empty">{flowEmpty}</div>
              {/if}
            </div>
          {/if}
          {#if diffTab}
            <div class="pane">
              {#key `${diffTab.rev}:${diffTab.path}`}
                <DiffView path={diffTab.path} rev={diffTab.rev} />
              {/key}
            </div>
          {/if}
          {#if rebaseTab}
            <div class="pane">
              {#key rebaseTab.path}
                <RebaseView path={rebaseTab.path} />
              {/key}
            </div>
          {/if}
          {#if activeKind === 'map' && app.map}
            <div class="pane">
              <ProjectMap
                map={app.map}
                files={app.info?.files ?? []}
                root={app.info?.root ?? ''}
                selected={app.selectedLabel}
                onselect={(n) => selectLabel(n)}
                onopen={openLabelGraph}
              />
            </div>
          {/if}
        </div>
        {#if (app.stageOpen || app.flowOpen) && !sceneMain && fileCenter}
          <Splitter axis="x" grow={-1} value={flowW} min={280} hardMax={800} fraction={0.45} reserve={240} reset={420} onchange={setFlow} />
          <section class="flow-pane" style={`width:${flowW}px`}>
            {#if app.stageOpen}
              <div class="stage-slot" style={app.flowOpen ? `flex: 0 0 ${stageH}px` : ''}>
                <StagePane />
              </div>
            {/if}
            {#if app.stageOpen && app.flowOpen}
              <Splitter axis="y" grow={1} value={stageH} min={80} hardMax={900} fraction={0.8} reserve={80} reset={240} onchange={setStage} />
            {/if}
            {#if app.flowOpen}
              <div class="flow-slot">
                {#if flowNode}
                  {#key flowNode.id}
                    <LabelGraph
                      name={flowNode.id}
                      file={flowFile}
                      cursorLine={cursorLine(flowFile)}
                      liveLine={liveLine(flowFile)}
                      reloadKey={app.changeSeq}
                      onopen={followFlowLabel}
                      ongoto={goTo}
                    />
                  {/key}
                {:else}
                  <div class="flow-empty">{flowEmpty}</div>
                {/if}
              </div>
            {/if}
          </section>
        {/if}
      </div>
    </section>
  </div>
  {#if app.bottomOpen}
    <Splitter axis="y" grow={-1} value={bottomH} min={120} hardMax={1600} fraction={0.7} reserve={180} reset={200} onchange={setBottom} />
    <section class="bottom" class:max={app.bottomMax} style={app.bottomMax ? '' : `height:${bottomH}px`}>
      <BottomPanel />
    </section>
  {/if}
</div>

{#if drag}
  {@const dragged = app.editorTabs.find((t) => editorTabId(t) === drag?.id)}
  {#if dragged}
    <div class="tab-ghost" style={`left:${drag.x + 12}px;top:${drag.y + 8}px`} aria-hidden="true">
      <Icon name={tabIcon(dragged)} size={14} />
      <span>{tabTitle(dragged)}</span>
    </div>
  {/if}
{/if}

<style>
  .workspace {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .body {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  aside {
    flex: none;
    min-width: 0;
    border-right: 1px solid var(--line);
    display: flex;
    flex-direction: column;
  }
  aside.closed {
    display: none;
  }
  .editor {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .tabbar {
    display: flex;
    align-items: stretch;
    height: var(--h-bar);
    background: var(--panel);
    border-bottom: 1px solid var(--line);
    min-width: 0;
    flex: none;
  }
  .tabs {
    position: relative;
    display: flex;
    overflow-x: auto;
    overscroll-behavior-x: contain;
    flex: 0 1 auto;
    min-width: 0;
    align-items: stretch;
    scrollbar-width: thin;
    scrollbar-color: var(--line) transparent;
  }
  .tabs::-webkit-scrollbar {
    height: 4px;
  }
  .tabs::-webkit-scrollbar-thumb {
    background: var(--line);
    border-radius: 4px;
  }
  .tabs::-webkit-scrollbar-track {
    background: transparent;
  }
  .tabbar.fade-left .tabs {
    mask-image: linear-gradient(to right, transparent, black 16px);
  }
  .tabbar.fade-right .tabs {
    mask-image: linear-gradient(to left, transparent, black 16px);
  }
  .tabbar.fade-left.fade-right .tabs {
    mask-image: linear-gradient(to right, transparent, black 16px, black calc(100% - 16px), transparent);
  }
  .modes {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    flex: none;
    padding: 0 var(--sp-3);
    border-left: 1px solid var(--line);
  }
  .seg {
    display: inline-flex;
    padding: 2px;
    gap: 2px;
    background: var(--panel-2);
    border: 1px solid var(--line);
    border-radius: var(--r-md);
  }
  .seg-btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 22px;
    padding: 0 9px;
    border: none;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--dim);
    font-size: var(--fs-md);
  }
  .seg-btn:hover:not(.on) {
    color: var(--text);
    background: var(--hover);
    border-color: transparent;
  }
  .seg-btn.on {
    color: var(--text);
    background: var(--panel);
    box-shadow: var(--shadow-sm);
  }
  .side {
    display: flex;
    gap: var(--sp-1);
  }
  .side .icon.on {
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 16%, transparent);
  }
  .tab {
    flex: none;
    position: relative;
    border: none;
    border-radius: 0;
    background: transparent;
    color: var(--dim);
    white-space: nowrap;
    display: flex;
    align-items: center;
    padding: 0 4px 0 0;
    border-right: 1px solid var(--line-soft);
  }
  .tab {
    user-select: none;
  }
  .tab.dragging {
    opacity: 0.45;
  }
  .tab.pinned .tl {
    padding: 0 2px 0 10px;
  }
  .tab.preview .tt {
    font-style: italic;
  }
  .tab.pinned .x:not(.dirty) .glyph {
    opacity: 0.65;
    display: inline-grid;
  }
  .tab.pinned .x:not(.dirty):hover .glyph {
    opacity: 1;
  }
  .drop-mark {
    position: absolute;
    top: 3px;
    bottom: 3px;
    width: 2px;
    margin-left: -1px;
    border-radius: 1px;
    background: var(--accent);
    pointer-events: none;
    z-index: 1;
  }
  .tab-ghost {
    position: fixed;
    z-index: var(--z-context);
    display: flex;
    align-items: center;
    gap: 6px;
    max-width: 260px;
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
  .tab-ghost span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tablist {
    align-self: center;
    margin: 0 var(--sp-2);
    flex: none;
  }
  .tab:hover:not(.on) {
    background: var(--hover);
    color: var(--text);
  }
  .tab .tl,
  .tab .x {
    border: none;
    background: transparent;
    border-radius: 0;
    color: inherit;
  }
  .tab .tl {
    padding: 0 4px 0 12px;
    height: 100%;
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: var(--fs-md);
  }
  .ti {
    display: inline-grid;
    opacity: 0.8;
  }
  .tt {
    max-width: 200px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .livedot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--ok);
    flex: none;
  }
  .tab .x {
    padding: 0;
    width: 20px;
    height: 20px;
    display: grid;
    place-items: center;
    border-radius: var(--r-sm);
  }
  .tab .x:hover {
    background: var(--active);
  }
  .x .dot,
  .x .glyph {
    display: none;
  }
  .x.dirty .dot {
    display: inline-block;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
  }
  .tab .x .glyph {
    opacity: 0;
    display: inline-grid;
  }
  .tab:hover .x .glyph,
  .tab.on .x .glyph,
  .x:focus-visible .glyph {
    opacity: 1;
  }
  .x.dirty .glyph {
    display: none;
  }
  .tab:hover .x.dirty .glyph,
  .x.dirty:focus-visible .glyph {
    display: inline-grid;
  }
  .tab:hover .x.dirty .dot,
  .x.dirty:focus-visible .dot {
    display: none;
  }
  .tab.on {
    color: var(--text);
    background: var(--bg-code);
    box-shadow: inset 0 2px 0 var(--accent);
  }
  .tab.on .ti {
    opacity: 1;
  }
  .spacer {
    flex: 1;
    min-width: 8px;
  }
  .editor-row {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .stage {
    flex: 1;
    min-width: 0;
    position: relative;
    background: var(--bg-canvas);
  }
  .pane {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
  }
  .pane.hidden {
    visibility: hidden;
    pointer-events: none;
  }
  .empty-workspace {
    z-index: 1;
  }
  .empty-actions {
    display: flex;
    gap: var(--sp-3);
    flex-wrap: wrap;
    justify-content: center;
    margin-bottom: var(--sp-3);
  }
  .shortcuts {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 6px;
    font-size: var(--fs-md);
  }
  .shortcuts li {
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .shortcuts li :global(.kbd:last-of-type) {
    margin-right: var(--sp-3);
  }
  .flow-pane {
    flex: none;
    min-width: 0;
    min-height: 0;
    height: 100%;
    border-left: 1px solid var(--line);
    position: relative;
    background: var(--bg-canvas);
    display: flex;
    flex-direction: column;
  }
  .stage-slot {
    flex: 1 1 auto;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    border-bottom: 1px solid var(--line);
  }
  .flow-pane:not(:has(.flow-slot)) .stage-slot {
    flex: 1 1 auto;
  }
  .flow-slot {
    flex: 1;
    min-height: 0;
    position: relative;
    overflow: hidden;
  }
  .flow-slot :global(.lg) {
    height: 100%;
  }
  .flow-empty {
    height: 100%;
    display: grid;
    place-items: center;
    color: var(--dim);
    padding: 16px;
    text-align: center;
  }
  .bottom {
    flex: none;
    min-height: 0;
    border-top: 1px solid var(--line);
  }
  .bottom.max {
    flex: 3 1 0;
    height: auto;
  }
</style>
