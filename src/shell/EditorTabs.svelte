<script lang="ts">
  import { onDestroy, untrack } from 'svelte'
  import Icon from '../components/Icon.svelte'
  import { openContextMenu, openMenuBelow, tabBarItems, tabItems, tabListItems } from '../lib/context.svelte'
  import { createTabDrag, horizontalWheel } from '../lib/tabdrag.svelte'
  import { tabIcon, tabIsLive, tabTitle } from '../lib/tabs'
  import { settings } from '../lib/settings.svelte'
  import {
    activateEditor,
    app,
    closeEditor,
    editorTabId,
    isPinned,
    moveEditor,
    promoteTab,
    unpinEditor,
  } from '../lib/store.svelte'
  import { showCode, toggleFlow, toggleScene, toggleStage } from '../lib/nav.svelte'

  let tabsEl: HTMLElement | undefined = $state()
  let fadeLeft = $state(false)
  let fadeRight = $state(false)
  let listBtn: HTMLButtonElement | undefined = $state()

  const drag = createTabDrag({
    strip: () => tabsEl,
    indexOf: (id) => app.editorTabs.findIndex((t) => editorTabId(t) === id),
    activate: activateEditor,
    move: moveEditor,
  })
  onDestroy(() => drag.stop())

  function tabScroll() {
    const node = tabsEl
    if (!node) return
    fadeLeft = node.scrollLeft > 4
    fadeRight = node.scrollLeft + node.clientWidth < node.scrollWidth - 4
  }

  $effect(() => {
    const on = settings.previewTabs
    const preview = app.previewTab
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

  /** Alt+Left and Alt+Right move the focused tab one place. */
  function onTabKey(e: KeyboardEvent, id: string) {
    if (!e.altKey || (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight')) return
    const index = app.editorTabs.findIndex((t) => editorTabId(t) === id)
    if (index < 0) return
    e.preventDefault()
    const step = e.key === 'ArrowLeft' ? -1 : 1
    const neighbour = app.editorTabs[index + step]
    if (!neighbour || isPinned(editorTabId(neighbour)) !== isPinned(id)) return
    moveEditor(id, step < 0 ? index - 1 : index + 2)
  }

  const showTabList = $derived(app.editorTabs.length > 0 && (fadeLeft || fadeRight || app.editorTabs.length > 6))
  const activeTab = $derived(app.editorTabs.find((t) => editorTabId(t) === app.activeEditor) ?? null)
  const fileCenter = $derived(activeTab?.kind === 'file')
  const centerFlow = $derived(app.sceneExpanded && fileCenter && app.editorTabs.length > 0)

  function showCenterFlow() {
    if (!app.sceneExpanded) toggleScene()
  }
</script>

<div class="tabbar" class:fade-left={fadeLeft} class:fade-right={fadeRight}>
  <div class="tabs" bind:this={tabsEl} use:horizontalWheel onscroll={tabScroll} role="tablist" aria-label="Editors">
    {#each app.editorTabs as t (editorTabId(t))}
      {@const id = editorTabId(t)}
      {@const dirty = t.kind === 'file' && app.dirtyFiles.includes(t.path)}
      {@const pinned = isPinned(id)}
      {@const live = tabIsLive(t, app.live)}
      <div
        class="tab"
        role="presentation"
        class:on={app.activeEditor === id}
        class:live
        class:pinned
        class:preview={app.previewTab === id}
        class:dragging={drag.drag?.id === id}
        onpointerdown={(e) => drag.begin(e, id)}
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
          {#if live}<i class="livedot" title="Live" aria-label="Live"></i>{/if}
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
    {#if drag.drag && drag.drag.slot !== null}
      <i class="drop-mark" style={`left:${drag.drag.mark}px`} aria-hidden="true"></i>
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
          onclick={showCode}
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

{#if drag.drag}
  {@const dragged = app.editorTabs.find((t) => editorTabId(t) === drag.drag?.id)}
  {#if dragged}
    <div class="tab-ghost" style={`left:${drag.drag.x + 12}px;top:${drag.drag.y + 8}px`} aria-hidden="true">
      <Icon name={tabIcon(dragged)} size={14} />
      <span>{tabTitle(dragged)}</span>
    </div>
  {/if}
{/if}

<style>
  .tabbar {
    display: flex;
    align-items: stretch;
    height: var(--h-bar);
    background: var(--panel);
    border-bottom: 1px solid var(--line);
    min-width: 0;
    flex: none;
    position: relative;
    z-index: var(--z-raised);
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
  .tabs::-webkit-scrollbar { height: 4px; }
  .tabs::-webkit-scrollbar-thumb { background: var(--line); border-radius: var(--r-sm); }
  .tabs::-webkit-scrollbar-track { background: transparent; }
  .tabbar.fade-left .tabs { mask-image: linear-gradient(to right, transparent, black 16px); }
  .tabbar.fade-right .tabs { mask-image: linear-gradient(to left, transparent, black 16px); }
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
    padding: var(--sp-1);
    gap: var(--sp-1);
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
  .side { display: flex; gap: var(--sp-1); }
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
    padding: 0 var(--sp-2) 0 0;
    border-right: 1px solid var(--line-soft);
    user-select: none;
  }
  .tab.dragging { opacity: 0.45; }
  .tab.pinned .tl { padding: 0 var(--sp-1) 0 10px; }
  .tab.preview .tt { font-style: italic; }
  .tab.pinned .x:not(.dirty) .glyph { opacity: 0.65; display: inline-grid; }
  .tab.pinned .x:not(.dirty):hover .glyph { opacity: 1; }
  .drop-mark {
    position: absolute;
    top: 3px;
    bottom: 3px;
    width: 2px;
    margin-left: -1px;
    border-radius: 1px;
    background: var(--accent);
    pointer-events: none;
    z-index: var(--z-base);
  }
  .tab-ghost {
    position: fixed;
    z-index: var(--z-context);
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    max-width: 260px;
    padding: 3px var(--sp-3);
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
  .tablist { align-self: center; margin: 0 var(--sp-2); flex: none; }
  .tab:hover:not(.on) { background: var(--hover); color: var(--text); }
  .tab .tl, .tab .x { border: none; background: transparent; border-radius: 0; color: inherit; }
  .tab .tl {
    padding: 0 var(--sp-2) 0 var(--sp-4);
    height: 100%;
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    font-size: var(--fs-md);
  }
  .ti { display: inline-grid; opacity: 0.8; }
  .tt { max-width: 200px; overflow: hidden; text-overflow: ellipsis; }
  .livedot { width: 6px; height: 6px; border-radius: 50%; background: var(--ok); flex: none; }
  .tab .x {
    padding: 0;
    width: 20px;
    height: 20px;
    display: grid;
    place-items: center;
    border-radius: var(--r-sm);
  }
  .tab .x:hover { background: var(--active); }
  .x .dot, .x .glyph { display: none; }
  .x.dirty .dot { display: inline-block; width: 7px; height: 7px; border-radius: 50%; background: var(--accent); }
  .tab .x .glyph { opacity: 0; display: inline-grid; }
  .tab:hover .x .glyph, .tab.on .x .glyph, .x:focus-visible .glyph { opacity: 1; }
  .x.dirty .glyph { display: none; }
  .tab:hover .x.dirty .glyph, .x.dirty:focus-visible .glyph { display: inline-grid; }
  .tab:hover .x.dirty .dot, .x.dirty:focus-visible .dot { display: none; }
  .tab.on {
    color: var(--text);
    background: var(--bg-code);
    box-shadow: inset 0 2px 0 var(--accent);
  }
  .tab.on .ti { opacity: 1; }
  .spacer { flex: 1; min-width: var(--sp-3); }
</style>
