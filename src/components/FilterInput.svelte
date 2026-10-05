<script lang="ts">
  import Icon from './Icon.svelte'

  interface Props {
    value: string
    placeholder: string
    label?: string
    oninput?: () => void
  }

  let { value = $bindable(), placeholder, label, oninput }: Props = $props()
</script>

<label class="filter">
  <Icon name="search" size={13} />
  <input
    type="text"
    spellcheck="false"
    {placeholder}
    aria-label={label ?? placeholder}
    bind:value
    {oninput}
    onkeydown={(e) => {
      if (e.key === 'Escape' && value) {
        e.stopPropagation()
        value = ''
        oninput?.()
      }
    }}
  />
  {#if value}
    <button
      type="button"
      class="clear icon sm ghost"
      aria-label="Clear filter"
      onclick={() => {
        value = ''
        oninput?.()
      }}
    >
      <Icon name="close" size={12} />
    </button>
  {/if}
</label>

<style>
  .filter {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 4px 0 8px;
    height: var(--h-control);
    background: var(--bg);
    border: 1px solid var(--line);
    border-radius: var(--r-md);
    color: var(--dim);
    min-width: 0;
    flex: 1 1 120px;
  }
  .filter:focus-within {
    border-color: var(--accent);
    box-shadow: 0 0 0 1px var(--focus-ring);
  }
  input {
    flex: 1;
    min-width: 0;
    width: auto;
    padding: 0;
    border: none;
    background: transparent;
    height: 100%;
  }
  input:focus {
    outline: none;
  }
  .clear {
    width: 18px;
    height: 18px;
  }
</style>
