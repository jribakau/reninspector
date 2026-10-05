<script lang="ts">
  import { onMount } from 'svelte'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import Icon from '../components/Icon.svelte'

  let maximized = $state(false)

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

<div class="wins">
  <button type="button" class="win" aria-label="Minimize" onclick={() => void minimize()}>
    <Icon name="minimize" />
  </button>
  <button type="button" class="win" aria-label={maximized ? 'Restore' : 'Maximize'} onclick={() => void toggleMax()}>
    <Icon name={maximized ? 'restore' : 'maximize'} />
  </button>
  <button type="button" class="win close" aria-label="Close window" onclick={() => void closeWindow()}>
    <Icon name="window-close" />
  </button>
</div>

<style>
  .wins {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    margin-left: var(--sp-2);
  }
  .win {
    width: 28px;
    height: 26px;
    flex: none;
    padding: 0;
    display: grid;
    place-items: center;
    border: none;
    background: transparent;
    color: var(--dim);
    border-radius: var(--r-md);
  }
  .win:hover:not(:disabled) {
    background: var(--hover);
    border-color: transparent;
    color: var(--text);
  }
  .win:active:not(:disabled) {
    background: var(--active);
  }
  .win.close:hover:not(:disabled) {
    background: var(--error);
    color: var(--on-error);
  }
  .win.close:active:not(:disabled) {
    background: var(--error);
    filter: brightness(0.9);
  }
</style>
