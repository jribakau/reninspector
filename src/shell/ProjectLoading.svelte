<script lang="ts">
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
    animation: fade 120ms ease-out;
  }

  .card {
    display: grid;
    justify-items: center;
    gap: 6px;
    min-width: 220px;
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
    margin-top: 8px;
    border-radius: var(--r-pill);
    background: var(--line);
    overflow: hidden;
  }

  .track span {
    display: block;
    width: 40%;
    height: 100%;
    background: var(--accent);
    animation: slide 1.1s ease-in-out infinite;
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
