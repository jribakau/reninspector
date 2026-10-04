<script lang="ts">
  import FilterInput from './FilterInput.svelte'
  import { ROW } from '../lib/view'
  import { onMount } from 'svelte'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import { api, pickImage } from '../lib/api'
  import { copyText, labelItems, openContextMenu, placeItems } from '../lib/context.svelte'
  import { askText } from '../lib/dialog.svelte'
  import { sceneEdit } from '../lib/scene.svelte'
  import { app, goTo, searchDialogue, symbolsOf, type RenpySection } from '../lib/store.svelte'
  import type { AssetReport, Symbol } from '../lib/types'
  import ArchivesPanel from './ArchivesPanel.svelte'
  import AssetsPanel from './AssetsPanel.svelte'
  import TranslatePanel from './TranslatePanel.svelte'
  import VarsPanel from './VarsPanel.svelte'
  import VirtualList from './VirtualList.svelte'

  const allSections: { id: RenpySection; label: string }[] = [
    { id: 'characters', label: 'Characters' },
    { id: 'images', label: 'Images' },
    { id: 'screens', label: 'Screens' },
    { id: 'variables', label: 'Variables' },
    { id: 'languages', label: 'Languages' },
    { id: 'assets', label: 'Assets' },
    { id: 'archives', label: 'Archives' },
  ]
  const sections = allSections

  let symbolFilter = $state('')
  let charName = $state('')
  let charWho = $state('')
  /** The color Ren'Py's own character docs use for a new speaker's name. Written into the script, not used by the UI. */
  const DEFAULT_CHARACTER_COLOR = '#c8ffc8'
  let charColor = $state(DEFAULT_CHARACTER_COLOR)
  let assets = $state<AssetReport | null>(null)

  onMount(() => {
    let stop: (() => void) | undefined
    void getCurrentWindow()
      .listen<{ type?: string; paths?: string[] }>('tauri://drag-drop', (event) => {
        const payload = event.payload
        if (payload?.type && payload.type !== 'drop') return
        if (app.renpySection !== 'images') return
        for (const path of payload?.paths ?? []) void addImageFile(path)
      })
      .then((un) => {
        stop = un
      })
      .catch(() => {})
    return () => stop?.()
  })

  $effect(() => {
    const section = app.renpySection
    void app.changeSeq
    if (section !== 'characters' && section !== 'images') return
    let cancel = false
    void api.assetReport().then((report) => {
      if (!cancel) assets = report
    }).catch(() => {
      if (!cancel) assets = null
    })
    return () => {
      cancel = true
    }
  })

  function artNote(name: string): string {
    const n = name.toLowerCase()
    const missing = (assets?.missing ?? [])
      .filter((m) => m.what.toLowerCase().includes(n) || m.path.toLowerCase().includes(n))
      .map((m) => m.path)
    const unused = (assets?.files ?? [])
      .filter((f) => !f.used && f.path.toLowerCase().includes(n))
      .map((f) => f.path)
    const parts: string[] = []
    if (missing.length) parts.push(`missing ${missing.slice(0, 2).join(', ')}`)
    if (unused.length) parts.push(`unused ${unused.slice(0, 2).join(', ')}`)
    return parts.join(' · ')
  }

  async function createCharacter() {
    const name = charName.trim()
    if (!name) return
    const ok = await sceneEdit({
      op: 'add-character',
      path: 'characters.rpy',
      name,
      text: charWho.trim() || name,
      color: charColor.trim(),
    })
    if (ok) {
      charName = ''
      charWho = ''
    }
  }

  async function addPortrait(varName: string) {
    const source = await pickImage()
    if (!source) return
    await sceneEdit({
      op: 'add-image',
      path: 'images.rpy',
      name: `side ${varName}`,
      source,
    })
  }

  async function addImageFile(source: string) {
    const base = source.split(/[/\\]/).pop()?.replace(/\.[^.]+$/, '') ?? 'image'
    const suggested = base.replace(/[^\w]+/g, ' ').trim() || 'image'
    const name = await askText('Image name', suggested, 'Add')
    if (!name) return
    await sceneEdit({
      op: 'add-image',
      path: 'images.rpy',
      name: name.trim(),
      source,
    })
  }

  async function chooseImage() {
    const source = await pickImage()
    if (source) await addImageFile(source)
  }

  const speakers = $derived(app.catalog?.dialogue.speakers ?? [])
  const characters = $derived(symbolsOf('character'))
  const shownSpeakers = $derived.by(() => {
    const q = symbolFilter.trim().toLowerCase()
    return speakers.filter((s) => !q || s.name.toLowerCase().includes(q))
  })
  const shownCharacters = $derived.by(() => {
    const q = symbolFilter.trim().toLowerCase()
    return characters.filter((s) => !q || s.name.toLowerCase().includes(q))
  })

  function defineOf(name: string): Symbol | undefined {
    return characters.find((c) => c.name === name && c.path)
  }

  const symbolKind = $derived(app.renpySection === 'images' ? 'image' : app.renpySection === 'screens' ? 'screen' : '')
  const symbols = $derived.by(() => {
    if (!symbolKind) return [] as Symbol[]
    const q = symbolFilter.trim().toLowerCase()
    return symbolsOf(symbolKind).filter((s) => !q || s.name.toLowerCase().includes(q))
  })
</script>

<div class="sb-panel">
  <div class="sb-head"><h2>Ren'Py</h2></div>
  <div class="sb-tools">
    <div class="sb-subnav" role="group" aria-label="Ren'Py sections">
      {#each sections as s (s.id)}
        <button class:on={app.renpySection === s.id} aria-pressed={app.renpySection === s.id} onclick={() => (app.renpySection = s.id)}>
          {s.label}
        </button>
      {/each}
    </div>
  </div>
  <div class="section">
    <div class="keep" class:off={app.renpySection !== 'characters'}>
      <div class="sb-tools">
        <FilterInput placeholder="Filter characters…" bind:value={symbolFilter} />
      </div>
      <form class="sb-tools" onsubmit={(e) => { e.preventDefault(); void createCharacter() }}>
        <input placeholder="Name in the script" bind:value={charName} aria-label="Character name" />
        <input placeholder="Name on screen" bind:value={charWho} aria-label="Name on screen" />
        <input placeholder={DEFAULT_CHARACTER_COLOR} bind:value={charColor} aria-label="Color" />
        <button type="submit">New character</button>
      </form>
      {#if speakers.length}
        <div class="sb-list virtual">
          <VirtualList items={shownSpeakers} rowHeight={ROW.md}>
            {#snippet row(s)}
              {@const defined = defineOf(s.name)}
              <div
                class="sb-item speaker"
                role="button"
                tabindex="0"
                title="Search this character's dialogue"
                onclick={() => searchDialogue(s.name)}
                onkeydown={(e) => { if (e.key === 'Enter') searchDialogue(s.name) }}
                oncontextmenu={(e) =>
                  openContextMenu(e, [
                    { kind: 'item', label: 'Find dialogue', run: () => searchDialogue(s.name) },
                    { kind: 'item', label: 'Open definition', enabled: !!defined, run: () => defined && goTo(defined.path, defined.line) },
                    { kind: 'item', label: 'Portrait', enabled: !!defined, run: () => void addPortrait(s.name) },
                    { kind: 'item', label: 'Copy name', run: () => copyText(s.name) },
                  ])}
              >
                <span class="sb-name">{s.name}</span>
                {#if defined}
                  <button
                    class="define"
                    title="Open the character definition"
                    onclick={(e) => { e.stopPropagation(); goTo(defined.path, defined.line) }}
                  >Define</button>
                  <button
                    class="define"
                    title="Use a picture as this character's side image"
                    onclick={(e) => { e.stopPropagation(); void addPortrait(s.name) }}
                  >Portrait</button>
                {/if}
                <span class="sb-meta">
                  {s.lines.toLocaleString()} lines · {s.words.toLocaleString()} words
                  {#if artNote(s.name)} · {artNote(s.name)}{/if}
                </span>
              </div>
            {/snippet}
          </VirtualList>
        </div>
        {#if !shownSpeakers.length}<div class="sb-empty">Nothing matches.</div>{/if}
      {:else if characters.length}
        <div class="sb-list virtual">
          <VirtualList items={shownCharacters} rowHeight={ROW.md}>
            {#snippet row(s)}
              <div class="sb-item speaker">
                <button
                  class="name"
                  onclick={() => s.path && goTo(s.path, s.line)}
                  oncontextmenu={(e) =>
                    openContextMenu(e, [
                      { kind: 'item', label: 'Open', enabled: !!s.path, run: () => s.path && goTo(s.path, s.line) },
                      { kind: 'item', label: 'Portrait', run: () => void addPortrait(s.name) },
                      { kind: 'item', label: 'Copy name', run: () => copyText(s.name) },
                    ])}
                >
                  <span class="sb-name">{s.name}</span>
                  <span class="sb-meta">{s.detail || `${s.path}:${s.line}`}{artNote(s.name) ? ` · ${artNote(s.name)}` : ''}</span>
                </button>
                <button class="define" title="Use a picture as this character's side image" onclick={() => void addPortrait(s.name)}>Portrait</button>
              </div>
            {/snippet}
          </VirtualList>
        </div>
      {:else}
        <div class="sb-empty">No characters in the readable scripts.</div>
      {/if}
    </div>
    <div class="keep" class:off={app.renpySection !== 'images' && app.renpySection !== 'screens'}>
      <div class="sb-tools">
        <FilterInput placeholder={app.renpySection === 'images' ? 'Filter images…' : 'Filter screens…'} bind:value={symbolFilter} />
        {#if app.renpySection === 'images'}
          <button type="button" onclick={() => void chooseImage()}>Add image…</button>
          <span class="sb-meta">Drop a png, jpeg, webp, or gif here. It is copied into images/ and an image statement is written.</span>
        {/if}
      </div>
      <div class="sb-list virtual">
        <VirtualList items={symbols} rowHeight={ROW.md}>
          {#snippet row(s)}
            <button
              class="sb-item"
              onclick={() => s.path && goTo(s.path, s.line)}
              oncontextmenu={(e) =>
                openContextMenu(
                  e,
                  app.renpySection === 'screens'
                    ? labelItems(`screen:${s.name}`)
                    : s.path
                      ? placeItems(s.path, s.line, s.name)
                      : [{ kind: 'item', label: 'Copy name', run: () => copyText(s.name) }],
                )}
            >
              <span class="sb-name">{s.name}</span>
              <span class="sb-meta">{s.detail || `${s.path}:${s.line}`}{app.renpySection === 'images' && artNote(s.name) ? ` · ${artNote(s.name)}` : ''}</span>
            </button>
          {/snippet}
        </VirtualList>
      </div>
      {#if !symbols.length}<div class="sb-empty">Nothing matches.</div>{/if}
    </div>
    <div class="keep" class:off={app.renpySection !== 'variables'}><VarsPanel /></div>
    <div class="keep" class:off={app.renpySection !== 'languages'}><TranslatePanel /></div>
    <div class="keep" class:off={app.renpySection !== 'assets'}><AssetsPanel /></div>
    <div class="keep" class:off={app.renpySection !== 'archives'}><ArchivesPanel /></div>
  </div>
</div>

<style>
  .keep {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }
  .keep.off {
    display: none;
  }
  .section {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .speaker {
    flex-direction: row;
    flex-wrap: wrap;
    align-items: center;
    column-gap: 6px;
    cursor: pointer;
  }
  .speaker .sb-meta {
    flex: 1 0 100%;
  }
  .define {
    flex: none;
    font-size: var(--fs-xs);
    padding: 0 6px;
    height: 18px;
  }
  .speaker .name {
    flex: 1;
    min-width: 0;
    background: none;
    border: none;
    color: inherit;
    text-align: left;
    padding: 0;
  }
</style>
