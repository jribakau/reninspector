<script lang="ts">
  import { errorText, readPreview } from '../lib/api'
  import { previewMime, type PreviewKind } from '../lib/preview'
  import Icon from './Icon.svelte'
  import ZoomControls from './ZoomControls.svelte'

  let { path, kind }: { path: string; kind: PreviewKind } = $props()

  let url = $state('')
  let err = $state('')
  let loading = $state(true)
  let zoom = $state(1)
  let natural = $state('')

  const name = $derived(path.split('/').pop() ?? path)

  $effect(() => {
    const file = path
    const view = kind
    let cancel = false
    let blob = ''
    url = ''
    err = ''
    loading = true
    zoom = 1
    natural = ''
    void readPreview(file)
      .then((buf) => {
        const next = URL.createObjectURL(new Blob([buf], { type: previewMime(file, view) }))
        if (cancel) {
          URL.revokeObjectURL(next)
          return
        }
        blob = next
        url = next
      })
      .catch((e) => {
        if (!cancel) err = errorText(e)
      })
      .finally(() => {
        if (!cancel) loading = false
      })
    return () => {
      cancel = true
      if (blob) URL.revokeObjectURL(blob)
    }
  })

  function sized(e: Event) {
    const img = e.currentTarget
    if (img instanceof HTMLImageElement && img.naturalWidth) {
      natural = `${img.naturalWidth} × ${img.naturalHeight}`
    }
  }
</script>

<div class="view">
  <div class="head pane-head">
    <Icon name={kind === 'image' ? 'image' : kind === 'audio' ? 'audio' : 'video'} size={14} />
    <span class="name" title={path}>{name}</span>
    {#if kind === 'image' && natural}<span class="meta">{natural}</span>{/if}
    <span class="grow"></span>
    {#if kind === 'image' && url}
      <ZoomControls
        k={zoom}
        onfit={() => (zoom = 1)}
        onin={() => (zoom = Math.min(6, zoom * 1.25))}
        onout={() => (zoom = Math.max(0.25, zoom / 1.25))}
      />
    {/if}
  </div>
  <div class="stage">
    {#if loading && !err}
      <p class="meta">Loading…</p>
    {:else if err}
      <p class="err">{err}</p>
    {:else if url && kind === 'image'}
      <img src={url} alt={name} style={`transform: scale(${zoom})`} onload={sized} />
    {:else if url && kind === 'audio'}
      <audio controls src={url}></audio>
    {:else if url && kind === 'video'}
      <!-- svelte-ignore a11y_media_has_caption -->
      <video controls src={url}></video>
    {/if}
  </div>
</div>

<style>
  .view {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    background: var(--bg-code);
  }
  .name {
    font-weight: 600;
    color: var(--text);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .grow {
    flex: 1;
  }
  .meta {
    color: var(--dim);
    font-size: var(--fs-md);
  }
  .stage {
    flex: 1;
    min-height: 0;
    display: grid;
    place-items: center;
    overflow: auto;
    padding: 24px;
  }
  img,
  video {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
    background-color: var(--bg-canvas);
    border-radius: var(--r-sm);
  }
  img {
    /* Checkerboard so transparent regions are visible. */
    background-image:
      linear-gradient(45deg, var(--line-soft) 25%, transparent 25%, transparent 75%, var(--line-soft) 75%),
      linear-gradient(45deg, var(--line-soft) 25%, transparent 25%, transparent 75%, var(--line-soft) 75%);
    background-size: 16px 16px;
    background-position: 0 0, 8px 8px;
  }
  img {
    transform-origin: center;
  }
  audio {
    width: min(480px, 100%);
  }
  .err {
    font-size: var(--fs-lg);
  }
</style>
