<script lang="ts">
  import { onMount, tick, untrack } from 'svelte'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import Icon from '../components/Icon.svelte'
  import { git, openBranchPicker } from '../lib/git.svelte'
  import {
    menuRequest,
    menuSections,
    type Action,
    type MenuNode,
    type MenuSection,
  } from '../lib/menus.svelte'
  import { toggleLive } from '../lib/live.svelte'
  import { app } from '../lib/model.svelte'
  import { overlayBus } from '../lib/overlay.svelte'
  import { openProject } from '../lib/project.svelte'
  import { openSettings } from '../lib/settings.svelte'

  type Shell = null | 'nav' | 'project' | 'run'

  let shell = $state<Shell>(null)
  let sectionId = $state<string | null>(null)
  let index = $state(0)
  let recent = $state(false)
  let recentIndex = $state(0)
  let maximized = $state(false)

  const sections = $derived(menuSections())
  const section = $derived(sections.find((s) => s.id === sectionId) ?? null)
  const runSection = $derived(sections.find((s) => s.id === 'run') ?? null)
  const canRun = $derived(!!app.info && !app.busy)

  const projectTitle = $derived.by(() => {
    if (!app.info) return 'Open a project'
    const engine = app.info.engineVersion ? `Ren'Py ${app.info.engineVersion}` : "Ren'Py"
    return `${app.info.name} — ${engine}\n${app.info.root}`
  })

  function closeShell() {
    shell = null
    sectionId = null
    recent = false
    index = 0
    recentIndex = 0
  }

  function toggle(next: Shell) {
    if (shell === next) closeShell()
    else {
      shell = next
      sectionId = null
      recent = false
      index = 0
      recentIndex = 0
    }
  }

  function baseName(path: string): string {
    const parts = path.split(/[/\\]/).filter(Boolean)
    return parts[parts.length - 1] ?? path
  }

  type Leaf = {
    key: string
    enabled: boolean
    recent?: boolean
    action?: Action
    section?: MenuSection
    path?: string
  }

  function focusable(nodes: MenuNode[]): Leaf[] {
    const rows: Leaf[] = []
    for (const node of nodes) {
      if (node.kind === 'sep') continue
      if (node.kind === 'recent') rows.push({ key: 'recent', enabled: true, recent: true })
      else rows.push({ key: node.action.id, enabled: node.action.enabled, action: node.action })
    }
    return rows
  }

  function projectNodes(): MenuNode[] {
    const file = sections.find((s) => s.id === 'file')
    const wanted = ['open-project', 'new-project']
    const found = new Map<string, MenuNode>()
    for (const node of file?.items ?? []) {
      if (node.kind === 'action' && wanted.includes(node.action.id)) found.set(node.action.id, node)
    }
    return wanted.map((id) => found.get(id)).filter((node): node is MenuNode => !!node)
  }

  function recentLeaves(): Leaf[] {
    if (!app.recent.length) return [{ key: 'empty', enabled: false }]
    return app.recent.map((path) => ({ key: path, enabled: !app.busy, path }))
  }

  function leaf(): Leaf[] {
    if (shell === 'project') return [...focusable(projectNodes()), ...recentLeaves()]
    if (shell === 'run') return focusable(runSection?.items ?? [])
    if (shell === 'nav' && recent) return recentLeaves()
    if (shell === 'nav' && section) return focusable(section.items)
    if (shell === 'nav') return sections.map((s) => ({ key: s.id, enabled: true, section: s }))
    return []
  }

  function move(delta: number) {
    const items = leaf()
    if (!items.length) return
    const usingRecent = shell === 'nav' && recent
    let i = usingRecent ? recentIndex : index
    for (let n = 0; n < items.length; n++) {
      i = (i + delta + items.length) % items.length
      if (items[i].enabled) {
        if (usingRecent) recentIndex = i
        else index = i
        return
      }
    }
  }

  function activate() {
    const items = leaf()
    const item = items[recent ? recentIndex : index]
    if (!item || !item.enabled) return
    if (item.section) {
      sectionId = item.section.id
      index = 0
      recent = false
      return
    }
    if (item.recent) {
      recent = true
      recentIndex = 0
      return
    }
    if (item.path) {
      const path = item.path
      closeShell()
      void openProject(path)
      return
    }
    closeShell()
    item.action?.run()
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault()
      e.stopPropagation()
      if (recent) recent = false
      else if (shell === 'nav' && sectionId) sectionId = null
      else closeShell()
      return
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      e.stopPropagation()
      move(1)
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      e.stopPropagation()
      move(-1)
    } else if (e.key === 'ArrowRight') {
      e.preventDefault()
      e.stopPropagation()
      const item = leaf()[index]
      if (shell === 'nav' && !sectionId && item?.section) {
        focusSection(item.section.id)
      } else if (shell === 'nav' && sectionId && !recent && item?.recent) {
        recent = true
        recentIndex = 0
      }
    } else if (e.key === 'ArrowLeft') {
      e.preventDefault()
      e.stopPropagation()
      if (recent) recent = false
      else if (shell === 'nav' && sectionId) {
        const at = sections.findIndex((s) => s.id === sectionId)
        sectionId = null
        index = Math.max(0, at)
      }
    } else if (e.key === 'Enter') {
      e.preventDefault()
      e.stopPropagation()
      activate()
    }
  }

  $effect(() => {
    if (app.palette) closeShell()
  })

  $effect(() => {
    const seq = menuRequest.seq
    if (!seq) return
    const id = { f: 'file', e: 'edit', v: 'view', g: 'go', r: 'run', h: 'help' }[menuRequest.letter]
    if (!id) return
    untrack(() => {
      shell = 'nav'
      focusSection(id)
    })
  })

  $effect(() => {
    if (!shell) return
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  })

  function applyMax(value: boolean) {
    maximized = value
    document.documentElement.classList.toggle('maximized', value)
  }

  function tauriWindow() {
    if (!('__TAURI_INTERNALS__' in window)) return null
    return getCurrentWindow()
  }

  async function minimize() {
    await tauriWindow()?.minimize()
  }

  async function toggleMax() {
    const win = tauriWindow()
    if (!win) return
    await win.toggleMaximize()
    applyMax(await win.isMaximized())
  }

  async function closeWindow() {
    await tauriWindow()?.close()
  }

  function focusSection(id: string) {
    sectionId = id
    recent = false
    recentIndex = 0
    const items = leaf()
    const at = items.findIndex((item) => item.enabled)
    index = at < 0 ? 0 : at
  }

  function hoverSection(id: string) {
    if (sectionId !== id) focusSection(id)
  }

  function play() {
    if (!canRun) return
    void toggleLive()
  }

  let seenDismiss = 0
  $effect(() => {
    const n = overlayBus.dismiss
    if (n === seenDismiss) return
    seenDismiss = n
    if (shell) closeShell()
  })

  function placeFlyouts() {
    const nodes = document.querySelectorAll<HTMLElement>('.bar .fly, .bar .nest')
    for (const node of nodes) {
      const anchor = node.parentElement
      if (!anchor) continue
      node.style.position = 'fixed'
      node.style.left = '0px'
      node.style.top = '0px'
      const host = anchor.getBoundingClientRect()
      const width = node.offsetWidth
      const maxH = Math.min(window.innerHeight * 0.7, 520)
      node.style.maxHeight = `${maxH}px`
      const height = Math.min(node.scrollHeight, maxH)
      let left = host.right - 4
      if (left + width > window.innerWidth - 8) left = Math.max(8, host.left - width + 4)
      let top = host.top
      if (top + height > window.innerHeight - 8) top = Math.max(8, window.innerHeight - 8 - height)
      node.style.left = `${left}px`
      node.style.top = `${top}px`
    }
  }

  $effect(() => {
    void shell
    void sectionId
    void recent
    if (!shell) return
    void tick().then(placeFlyouts)
    window.addEventListener('resize', placeFlyouts)
    return () => window.removeEventListener('resize', placeFlyouts)
  })

  onMount(() => {
    const win = tauriWindow()
    if (!win) return
    void win.isMaximized().then(applyMax)
    let unlisten: (() => void) | undefined
    void win.onResized(() => {
      void win.isMaximized().then(applyMax)
    }).then((fn) => {
      unlisten = fn
    })
    return () => {
      unlisten?.()
      document.documentElement.classList.remove('maximized')
    }
  })
</script>

{#snippet accessText(label: string, access: string)}
  {@const at = label.toLowerCase().indexOf(access.toLowerCase())}
  {#if at < 0}
    {label}
  {:else}
    {label.slice(0, at)}<u>{label[at]}</u>{label.slice(at + 1)}
  {/if}
{/snippet}

{#snippet mark(on: boolean)}
  <span class="mark" aria-hidden="true">
    {#if on}<Icon name="check" size={13} />{/if}
  </span>
{/snippet}

{#snippet rows(nodes: MenuNode[], active: number)}
  {#each nodes as node, i (node.kind === 'action' ? node.action.id : node.kind === 'sep' ? `sep-${i}` : 'recent')}
    {#if node.kind === 'sep'}
      <div class="menu-sep" role="separator"></div>
    {:else if node.kind === 'recent'}
      <div class="subanchor">
        <button
          type="button"
          class="menu-item"
          class:on={active === focusable(nodes).findIndex((row) => row.recent)}
          role="menuitem"
          aria-haspopup="menu"
          onmouseenter={() => {
            index = focusable(nodes).findIndex((row) => row.recent)
            recent = true
            recentIndex = 0
          }}
          onclick={() => {
            recent = true
            recentIndex = 0
          }}
        >
          {@render mark(false)}
          <span class="lab">Open Recent</span>
          <span class="menu-key"><Icon name="chevron-right" size={12} /></span>
        </button>
        {#if recent}
          <div class="menu pop nest" role="menu" aria-label="Open Recent">
            {#if app.recent.length}
              {#each app.recent as path, i (path)}
                <button
                  type="button"
                  class="menu-item"
                  class:on={recentIndex === i}
                  role="menuitem"
                  disabled={!!app.busy}
                  title={path}
                  onmouseenter={() => (recentIndex = i)}
                  onclick={() => {
                    if (app.busy) return
                    closeShell()
                    void openProject(path)
                  }}
                >
                  {@render mark(false)}
                  <span class="lab">{baseName(path)}</span>
                </button>
              {/each}
            {:else}
              <button type="button" class="menu-item" role="menuitem" disabled>
                {@render mark(false)}
                <span class="lab">No recent projects</span>
              </button>
            {/if}
          </div>
        {/if}
      </div>
    {:else}
      {@const at = focusable(nodes).findIndex((row) => row.key === node.action.id)}
      <button
        type="button"
        class="menu-item"
        class:on={active === at}
        role="menuitem"
        disabled={!node.action.enabled}
        title={node.action.title}
        onmouseenter={() => {
          index = at
          recent = false
        }}
        onclick={() => {
          if (!node.action.enabled) return
          closeShell()
          node.action.run()
        }}
      >
        {@render mark(node.action.checked)}
        <span class="lab">{node.action.label}</span>
        {#if node.action.shortcut}<span class="menu-key">{node.action.shortcut}</span>{/if}
      </button>
    {/if}
  {/each}
{/snippet}

<header class="bar" data-tauri-drag-region="deep">
  <div class="left">
    <div class="anchor">
      <button
        type="button"
        class="icon"
        class:on={shell === 'nav'}
        aria-label="Main menu"
        title="Main menu — Alt+F File, Alt+E Edit, Alt+V View, Alt+G Go, Alt+R Run, Alt+H Help"
        aria-expanded={shell === 'nav'}
        aria-haspopup="menu"
        onclick={() => toggle('nav')}
      >
        <Icon name="menu" />
      </button>
      {#if shell === 'nav'}
        <div class="menu pop" role="menu">
          {#each sections as s, i (s.id)}
            <button
              type="button"
              class="menu-item"
              class:on={sectionId === s.id || (!sectionId && index === i)}
              role="menuitem"
              aria-haspopup="menu"
              onmouseenter={() => hoverSection(s.id)}
              onclick={() => hoverSection(s.id)}
            >
              {@render mark(false)}
              <span class="lab">{@render accessText(s.label, s.access)}</span>
              <span class="menu-key"><Icon name="chevron-right" size={12} /></span>
            </button>
          {/each}
          {#if section}
            <div class="menu pop fly" role="menu" aria-label={section.label}>
              {@render rows(section.items, index)}
            </div>
          {/if}
        </div>
      {/if}
    </div>

    <div class="anchor">
      <button
        type="button"
        class="chip"
        class:on={shell === 'project'}
        title={projectTitle}
        aria-expanded={shell === 'project'}
        aria-haspopup="menu"
        onclick={() => toggle('project')}
      >
        <Icon name="folder" />
        <span class="name">{app.info?.name ?? 'Open project'}</span>
        <span class="caret"><Icon name="caret" size={12} /></span>
      </button>
      {#if shell === 'project'}
        {@const nodes = projectNodes()}
        {@const recentAt = focusable(nodes).length}
        <div class="menu pop" role="menu" aria-label="Project">
          {@render rows(nodes, index)}
          <div class="menu-sep" role="separator"></div>
          {#if app.recent.length}
            {#each app.recent as path, i (path)}
              <button
                type="button"
                class="menu-item"
                class:on={index === recentAt + i}
                role="menuitem"
                disabled={!!app.busy}
                title={path}
                onmouseenter={() => (index = recentAt + i)}
                onclick={() => {
                  if (app.busy) return
                  closeShell()
                  void openProject(path)
                }}
              >
                {@render mark(false)}
                <span class="lab">{baseName(path)}</span>
              </button>
            {/each}
          {:else}
            <button type="button" class="menu-item" role="menuitem" disabled>
              {@render mark(false)}
              <span class="lab">No recent projects</span>
            </button>
          {/if}
        </div>
      {/if}
    </div>

    {#if git.status?.branch}
      <button
        type="button"
        class="chip"
        title={git.status.upstream ? `${git.status.branch} · ${git.status.upstream}` : `Branch ${git.status.branch}`}
        onclick={openBranchPicker}
      >
        <Icon name="branch" />
        <span class="name">{git.status.branch}</span>
        {#if git.status.behind || git.status.ahead}
          <span class="sync">
            {#if git.status.behind}<span class="sync-n"><Icon name="arrow-down" size={11} />{git.status.behind}</span>{/if}
            {#if git.status.ahead}<span class="sync-n"><Icon name="arrow-up" size={11} />{git.status.ahead}</span>{/if}
          </span>
        {/if}
      </button>
    {/if}
  </div>

  <div class="center">
    <div class="anchor">
      <div class="run">
        <button
          type="button"
          class="run-label"
          class:on={shell === 'run'}
          disabled={!canRun}
          aria-expanded={shell === 'run'}
          aria-haspopup="menu"
          onclick={() => toggle('run')}
        >
          Run
          <span class="caret"><Icon name="caret" size={12} /></span>
        </button>
        <button
          type="button"
          class="run-go"
          disabled={!canRun}
          aria-label={app.live.running ? 'Stop live' : 'Start live'}
          title={app.live.running ? 'Stop live (F5)' : 'Start live (F5)'}
          onclick={play}
        >
          <Icon name={app.live.running ? 'stop' : 'play'} size={14} />
        </button>
      </div>
      {#if shell === 'run' && runSection}
        <div class="menu pop run-pop" role="menu" aria-label="Run">
          {@render rows(runSection.items, index)}
        </div>
      {/if}
    </div>
  </div>

  <div class="right">
    <button type="button" class="icon" aria-label="Command palette" title="Command palette" onclick={() => (app.palette = 'commands')}>
      <Icon name="search" />
    </button>
    <button type="button" class="icon" aria-label="Settings" title="Settings (Ctrl+,)" onclick={() => openSettings()}>
      <Icon name="settings" />
    </button>
    <div class="wins">
      <button type="button" class="win" aria-label="Minimize" onclick={() => void minimize()}>
        <Icon name="minimize" />
      </button>
      <button type="button" class="win" aria-label={maximized ? 'Restore' : 'Maximize'} onclick={() => void toggleMax()}>
        <Icon name={maximized ? 'restore' : 'maximize'} />
      </button>
      <button type="button" class="win close" aria-label="Close" onclick={() => void closeWindow()}>
        <Icon name="window-close" />
      </button>
    </div>
  </div>
</header>

{#if shell}
  <button type="button" class="back" aria-label="Close menu" onclick={closeShell}></button>
{/if}

<style>
  .bar {
    height: var(--h-title);
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    gap: var(--sp-3);
    padding-left: 6px;
    background: var(--bg);
    border-bottom: 1px solid var(--line);
    flex: none;
    position: relative;
    z-index: var(--z-menu);
    user-select: none;
  }
  .left, .right {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    min-width: 0;
  }
  .right {
    justify-content: flex-end;
  }
  .center {
    display: flex;
    justify-content: center;
  }
  .anchor,
  .subanchor {
    position: relative;
  }
  .anchor {
    display: flex;
    align-items: center;
  }
  .icon, .chip, .win, .run button {
    border: none;
    background: transparent;
    color: var(--text);
    border-radius: var(--r-md);
  }
  .icon, .chip, .win {
    height: 26px;
    color: var(--dim);
  }
  .icon, .win {
    width: 28px;
    flex: none;
    padding: 0;
    display: grid;
    place-items: center;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    max-width: 280px;
    padding: 0 8px;
  }
  .icon:hover:not(:disabled),
  .chip:hover:not(:disabled),
  .run button:hover:not(:disabled),
  .icon.on,
  .chip.on,
  .run button.on {
    background: var(--active);
    color: var(--text);
    border-color: transparent;
  }
  .sync {
    flex: none;
    display: inline-flex;
    gap: 4px;
    color: var(--dim);
    font-size: var(--fs-sm);
  }
  .sync-n {
    display: inline-flex;
    align-items: center;
    gap: 1px;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text);
    font-size: var(--fs-md);
  }
  .caret {
    display: inline-grid;
    color: var(--dim);
  }
  .run {
    display: flex;
    align-items: stretch;
    height: 26px;
    border: 1px solid var(--line);
    border-radius: var(--r-md);
    background: var(--panel);
    overflow: visible;
  }
  .run-label, .run-go {
    height: 24px;
    border-radius: 0;
    font-size: var(--fs-md);
    color: var(--text);
  }
  .run-label {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 0 6px 0 10px;
    border-radius: 5px 0 0 5px;
  }
  .run-go {
    width: 28px;
    padding: 0;
    display: grid;
    place-items: center;
    color: var(--ok);
    border-left: 1px solid var(--line);
    border-radius: 0 5px 5px 0;
  }
  .run-go:hover:not(:disabled) {
    background: color-mix(in srgb, var(--ok) 18%, transparent);
  }
  .wins {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    margin-left: 2px;
  }
  .win:hover:not(:disabled) {
    background: var(--active);
    border-color: transparent;
  }
  .win.close:hover:not(:disabled) {
    background: var(--error);
    color: var(--on-error);
  }
  .back {
    position: fixed;
    inset: 0;
    z-index: calc(var(--z-menu) - 1);
    border: none;
    background: transparent;
    padding: 0;
    border-radius: 0;
  }
  .pop {
    position: absolute;
    z-index: 2;
    top: calc(100% + 4px);
    left: 0;
    min-width: 240px;
    max-height: min(70vh, 520px);
    animation: pop-in var(--dur) var(--ease);
  }
  .fly,
  .nest {
    top: 0;
    left: calc(100% - 4px);
    z-index: 3;
  }
  .run-pop {
    left: 50%;
    transform: translateX(-50%);
    animation-name: run-pop-in;
  }
  @keyframes run-pop-in {
    from {
      opacity: 0;
      transform: translateX(-50%) translateY(-3px);
    }
  }
  u {
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .lab {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mark {
    width: 14px;
    height: 14px;
    flex: none;
    display: grid;
    place-items: center;
    color: var(--accent);
  }
</style>
