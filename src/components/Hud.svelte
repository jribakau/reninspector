<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    children: Snippet
    /** Pin to the top-left of the graph. Turn off to place it yourself. */
    pinned?: boolean
  }

  let { children, pinned = true }: Props = $props()
</script>

<div class="hud" class:pinned>
  {@render children()}
</div>

<style>
  .hud.pinned {
    position: absolute;
    top: 10px;
    left: 10px;
    right: 24px;
    width: fit-content;
    max-width: calc(100% - 34px);
    z-index: var(--z-raised);
  }
  .hud :global(.info) {
    color: var(--dim);
    font-size: var(--fs-sm);
    padding: 0 var(--sp-2);
  }
  .hud :global(select) {
    height: 22px;
    padding: 0 6px;
    font-size: var(--fs-sm);
    border-radius: var(--r-sm);
  }
  .hud :global(button.on) {
    color: var(--text);
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 18%, var(--panel-2));
  }
</style>
