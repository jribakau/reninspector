<script lang="ts">
  import Icon from './Icon.svelte'

  interface Props {
    /** Current zoom factor, 1 = 100%. */
    k: number
    onfit: () => void
    onin: () => void
    onout: () => void
  }

  let { k, onfit, onin, onout }: Props = $props()
</script>

<span class="zoom" role="group" aria-label="Zoom">
  <button type="button" class="icon sm ghost" onclick={onout} title="Zoom out" aria-label="Zoom out"><Icon name="minus" size={13} /></button>
  <span class="pct" aria-live="off">{Math.round(k * 100)}%</span>
  <button type="button" class="icon sm ghost" onclick={onin} title="Zoom in" aria-label="Zoom in"><Icon name="plus" size={13} /></button>
  <button type="button" class="icon sm ghost" onclick={onfit} title="Fit to window" aria-label="Fit to window"><Icon name="fit" size={13} /></button>
</span>

<style>
  .zoom {
    display: inline-flex;
    align-items: center;
    gap: 1px;
  }
  .pct {
    min-width: 34px;
    text-align: center;
    color: var(--dim);
    font-size: var(--fs-sm);
    font-variant-numeric: tabular-nums;
  }
</style>
