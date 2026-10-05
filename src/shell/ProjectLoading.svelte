<script lang="ts">
  import Icon from '../components/Icon.svelte'
  import { app } from '../lib/store.svelte'

  const name = $derived.by(() => {
    const path = app.opening
    if (!path) return ''
    const parts = path.replace(/\\/g, '/').split('/').filter(Boolean)
    return parts.at(-1) ?? path
  })
</script>

<div class="veil" role="status" aria-live="polite" aria-busy="true" aria-label={`Opening ${name}`}>
  <div class="card">
    <div class="mark" aria-hidden="true"><Icon name="map" size={22} /></div>
    <p class="name" title={app.opening ?? ''}>{name}</p>
    <p class="status">{app.busy || 'Opening project…'}</p>
    <div class="track" aria-hidden="true"><span></span></div>
  </div>
</div>

<style>
  .veil {
    position: fixed;
    inset: 0;
    z-index: var(--z-modal);
    display: grid;
    place-items: center;
    background: var(--bg);
    animation: fade var(--dur) var(--ease);
  }

  .card {
    display: grid;
    justify-items: center;
    gap: var(--sp-2);
    min-width: 220px;
  }

  .mark {
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    margin-bottom: var(--sp-2);
    border-radius: var(--r-lg);
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 16%, var(--panel));
    border: 1px solid color-mix(in srgb, var(--accent) 40%, var(--line));
  }

  .name {
    margin: 0;
    max-width: min(440px, 80vw);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-lg);
    font-weight: 600;
  }

  .status {
    margin: 0;
    min-height: 1.2em;
    color: var(--dim);
    font-size: var(--fs-md);
  }

  .track {
    width: 160px;
    height: 2px;
    margin-top: var(--sp-3);
    border-radius: var(--r-pill);
    background: var(--line);
    overflow: hidden;
  }

  .track span {
    display: block;
    width: 40%;
    height: 100%;
    background: var(--accent);
    animation: slide 1.1s var(--ease) infinite;
  }

  @keyframes fade {
    from {
      opacity: 0;
    }
  }

  @keyframes slide {
    from {
      transform: translateX(-120%);
    }
    to {
      transform: translateX(280%);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .veil,
    .track span {
      animation: none;
    }

    .track span {
      width: 100%;
      opacity: 0.7;
    }
  }
</style>
