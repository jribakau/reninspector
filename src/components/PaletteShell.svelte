<script lang="ts">
  import type { Snippet } from 'svelte'
  import Icon from './Icon.svelte'

  interface Props {
    label: string
    placeholder: string
    query: string
    box?: HTMLDivElement | null
    input?: HTMLInputElement | null
    error?: string
    onclose: () => void
    onkey: (e: KeyboardEvent) => void
    oninput?: () => void
    children: Snippet
  }

  let {
    label,
    placeholder,
    query = $bindable(),
    box = $bindable(null),
    input = $bindable(null),
    error = '',
    onclose,
    onkey,
    oninput,
    children,
  }: Props = $props()
</script>

<div class="back anim-fade" role="presentation" onclick={onclose}></div>
<div class="box" bind:this={box} role="dialog" aria-label={label} tabindex="-1" onkeydown={onkey}>
  <div class="field">
    <Icon name="search" size={14} />
    <input bind:this={input} {placeholder} bind:value={query} {oninput} aria-label={label} />
  </div>
  {#if error}<div class="err" role="alert">{error}</div>{/if}
  {@render children()}
</div>

<style>
  .back {
    position: fixed;
    inset: 0;
    background: var(--scrim);
    z-index: var(--z-palette);
  }
  .box {
    position: fixed;
    z-index: calc(var(--z-palette) + 1);
    top: 12%;
    left: 50%;
    transform: translateX(-50%);
    width: min(560px, calc(100% - 32px));
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--r-lg);
    overflow: hidden;
    box-shadow: var(--shadow-lg);
    animation: box-in var(--dur) var(--ease);
  }
  @keyframes box-in {
    from {
      opacity: 0;
      transform: translateX(-50%) translateY(-4px) scale(0.99);
    }
  }
  .field {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    padding: 0 var(--sp-4);
    border-bottom: 1px solid var(--line);
    color: var(--dim);
  }
  input {
    flex: 1;
    min-width: 0;
    border: none;
    border-radius: 0;
    padding: 11px 0;
    background: transparent;
    font-size: var(--fs-lg);
  }
  input:focus {
    outline: none;
  }
  .err {
    padding: 8px var(--sp-4);
    color: var(--error);
    font-size: var(--fs-md);
    border-bottom: 1px solid var(--line);
  }
</style>
