<script lang="ts" generics="T extends string">
  import type { Snippet } from 'svelte'

  interface Tab {
    id: T
    label: string
    count?: number
    bad?: boolean
    pulse?: boolean
  }

  interface Props {
    tabs: Tab[]
    active: T
    /** Prefix for the generated ids; panels use `${prefix}-panel-${id}`. */
    prefix: string
    label: string
    onselect: (id: T) => void
    tools?: Snippet
  }

  let { tabs, active, prefix, label, onselect, tools }: Props = $props()

  function move(e: KeyboardEvent, index: number) {
    if (e.key !== 'ArrowRight' && e.key !== 'ArrowLeft') return
    e.preventDefault()
    const next = tabs[(index + (e.key === 'ArrowRight' ? 1 : tabs.length - 1)) % tabs.length]
    onselect(next.id)
    document.getElementById(`${prefix}-tab-${next.id}`)?.focus()
  }
</script>

<div class="strip">
  <div class="tabs" role="tablist" aria-label={label}>
    {#each tabs as t, i (t.id)}
      <button
        id={`${prefix}-tab-${t.id}`}
        role="tab"
        aria-selected={active === t.id}
        aria-controls={`${prefix}-panel-${t.id}`}
        tabindex={active === t.id ? 0 : -1}
        class:on={active === t.id}
        onclick={() => onselect(t.id)}
        onkeydown={(e) => move(e, i)}
      >
        {#if t.pulse}<i class="pulse" aria-hidden="true"></i>{/if}
        {t.label}
        {#if t.count !== undefined}<span class="badge" class:bad={t.bad}>{t.count}</span>{/if}
      </button>
    {/each}
  </div>
  <span class="spacer"></span>
  {#if tools}
    <div class="tools">{@render tools()}</div>
  {/if}
</div>

<style>
  .strip {
    display: flex;
    align-items: stretch;
    min-height: var(--h-bar);
    border-bottom: 1px solid var(--line);
    background: var(--panel);
    flex: none;
  }
  .tabs {
    display: flex;
    align-items: stretch;
    min-width: 0;
  }
  .tabs button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    border: none;
    border-radius: 0;
    background: transparent;
    padding: 0 12px;
    color: var(--dim);
    border-bottom: 2px solid transparent;
    font-size: var(--fs-md);
  }
  .tabs button:hover:not(.on) {
    color: var(--text);
    background: var(--hover);
  }
  .tabs button.on {
    color: var(--text);
    border-bottom-color: var(--accent);
  }
  .spacer {
    flex: 1;
  }
  .tools {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    padding: 0 var(--sp-3);
  }
  .pulse {
    display: inline-block;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--ok);
    animation: pulse 1.6s ease-in-out infinite;
  }
  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }
</style>
