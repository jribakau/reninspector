<script lang="ts">
  import { onMount } from 'svelte'
  import { readAsset } from '../lib/api'
  import { copyText, openContextMenu, placeItems } from '../lib/context.svelte'
  import { app, goTo, searchDialogue, symbolsOf } from '../lib/store.svelte'

  const PICTURE = /\.(png|jpe?g|webp|gif)$/i

  let thumbs = $state<Record<string, string>>({})
  let failed = $state<Record<string, boolean>>({})
  let now = $state(Date.now())
  const stale = $derived(app.live.running && app.live.receivedAt > 0 && now - app.live.receivedAt > 5000)
  const urls = new Map<string, { path: string; url: string }>()
  let generation = 0
  let alive = true

  const waiting = $derived(
    app.live.running && !app.live.file && !app.live.line && !app.live.speaker && app.live.showing.length === 0,
  )

  const images = $derived.by(() => {
    const byName = new Map<string, { path: string; line: number; detail: string }>()
    for (const sym of symbolsOf('image')) {
      if (!byName.has(sym.name)) byName.set(sym.name, { path: sym.path, line: sym.line, detail: sym.detail })
    }
    return byName
  })

  function drop(tag: string) {
    const prev = urls.get(tag)
    if (!prev) return
    URL.revokeObjectURL(prev.url)
    urls.delete(tag)
  }

  $effect(() => {
    const tags = app.live.showing
    const known = images
    const seq = ++generation
    const keep = new Set<string>()
    for (const tag of tags) {
      const detail = known.get(tag)?.detail ?? ''
      if (PICTURE.test(detail)) keep.add(tag)
    }
    let changed = false
    for (const tag of [...urls.keys()]) {
      const detail = known.get(tag)?.detail ?? ''
      if (!keep.has(tag) || urls.get(tag)?.path !== detail) {
        drop(tag)
        changed = true
      }
    }
    if (changed) thumbs = Object.fromEntries([...urls].map(([tag, entry]) => [tag, entry.url]))
    for (const tag of keep) {
      const path = known.get(tag)?.detail
      if (!path || urls.has(tag)) continue
      void readAsset(path)
        .then((buf) => {
          if (!alive || seq !== generation || !app.live.showing.includes(tag)) return
          const current = images.get(tag)?.detail
          if (current !== path) return
          drop(tag)
          const url = URL.createObjectURL(new Blob([buf]))
          if (!alive) {
            URL.revokeObjectURL(url)
            return
          }
          urls.set(tag, { path, url })
          thumbs = Object.fromEntries([...urls].map(([name, entry]) => [name, entry.url]))
        })
        .catch(() => {
          if (!alive || seq !== generation) return
          failed = { ...failed, [tag]: true }
        })
    }
  })

  onMount(() => {
    const timer = setInterval(() => (now = Date.now()), 1000)
    return () => {
      clearInterval(timer)
      alive = false
      for (const tag of [...urls.keys()]) drop(tag)
    }
  })

  function openTag(tag: string) {
    const sym = images.get(tag)
    if (sym?.path) goTo(sym.path, sym.line)
  }
</script>

<div class="strip">
  {#if waiting}
    <span class="dim">Waiting for the game…</span>
  {:else}
    {#if app.live.speaker}
      <button
        class="who"
        onclick={() => searchDialogue(app.live.speaker)}
        title="Search this character's dialogue"
        oncontextmenu={(e) =>
          openContextMenu(e, [
            { kind: 'item', label: 'Find dialogue', run: () => searchDialogue(app.live.speaker) },
            { kind: 'item', label: 'Copy name', run: () => copyText(app.live.speaker) },
          ])}
      >
        {app.live.speaker}
      </button>
    {/if}
    {#if app.live.file}
      <button class="tag" title="Go to the live line" onclick={() => app.live.file && goTo(app.live.file, app.live.line)}>
        {app.live.file.split('/').pop()}:{app.live.line}
      </button>
    {/if}
    {#if stale}<span class="warn">No update</span>{/if}
    {#each app.live.showing as tag (tag)}
      {@const defined = images.has(tag)}
      {#if defined}
        <button
          class="tag"
          onclick={() => openTag(tag)}
          title="Open this image"
          oncontextmenu={(e) => {
            const sym = images.get(tag)
            openContextMenu(
              e,
              sym?.path
                ? placeItems(sym.path, sym.line, tag)
                : [{ kind: 'item', label: 'Copy name', run: () => copyText(tag) }],
            )
          }}
        >
          {#if thumbs[tag]}<img src={thumbs[tag]} alt={`Preview of ${tag}`} />{:else if failed[tag]}<span class="miss" title="Couldn't load this image">!</span>{/if}
          <span>{tag}</span>
        </button>
      {:else}
        <span class="tag plain">{tag}</span>
      {/if}
    {/each}
  {/if}
</div>

<style>
  .strip {
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 8px;
    overflow-x: auto;
    overflow-y: hidden;
    white-space: nowrap;
    background: var(--panel);
    border-bottom: 1px solid var(--line);
    mask-image: linear-gradient(to right, transparent, black 8px, black calc(100% - 12px), transparent);
  }
  .warn {
    color: var(--warning);
    font-size: var(--fs-sm);
    flex: none;
  }
  .miss {
    color: var(--warning);
    font-weight: 700;
  }
  .dim {
    color: var(--dim);
    font-size: var(--fs-md);
  }
  .who,
  .tag {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 22px;
    padding: 0 8px;
    border-radius: var(--r-pill);
    font-size: var(--fs-md);
  }
  .who {
    font-weight: 600;
  }
  .plain {
    color: var(--dim);
    background: var(--panel-2);
    border: 1px solid var(--line);
  }
  img {
    height: 18px;
    width: auto;
    max-width: 32px;
    object-fit: contain;
    border-radius: var(--r-sm);
  }
</style>
