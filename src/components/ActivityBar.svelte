<script lang="ts">
  import { git } from '../lib/git.svelte'
  import type { IconName } from '../lib/icons'
  import { app, openBottom, showActivity, toggleBottom, type Activity } from '../lib/store.svelte'
  import { tooltip } from '../lib/tooltip'
  import Icon from './Icon.svelte'

  const gitChanges = $derived(git.status?.changes.length ?? 0)

  const errors = $derived(app.diag?.errors ?? 0)
  const problems = $derived(errors + (app.diag?.warnings ?? 0))

  const all: { id: Activity; label: string; icon: IconName }[] = [
    { id: 'explorer', label: 'Explorer', icon: 'explorer' },
    { id: 'story', label: 'Story', icon: 'story' },
    { id: 'search', label: 'Search', icon: 'search' },
    { id: 'renpy', label: "Ren'Py", icon: 'renpy' },
    { id: 'git', label: 'Git', icon: 'branch' },
  ]

  function count(n: number): string {
    return n > 99 ? '99+' : String(n)
  }

  function showProblems() {
    if (app.bottomOpen && app.bottomTab === 'problems') toggleBottom()
    else openBottom('problems')
  }
</script>

<nav class="rail" aria-label="Activity">
  <div class="group">
    {#each all as item (item.id)}
      {@const active = app.sidebarOpen && app.activity === item.id}
      <button
        class:on={active}
        use:tooltip={item.label}
        aria-label={item.label}
        aria-current={active ? 'page' : undefined}
        onclick={() => showActivity(item.id)}
        onkeydown={(e) => {
          const index = all.findIndex((entry) => entry.id === item.id)
          if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return
          e.preventDefault()
          const next = all[(index + (e.key === 'ArrowDown' ? 1 : all.length - 1)) % all.length]
          showActivity(next.id)
        }}
      >
        <Icon name={item.icon} size={18} />
        {#if item.id === 'git' && gitChanges > 0}<span class="badge accent">{count(gitChanges)}</span>{/if}
      </button>
    {/each}
  </div>

  {#if app.info}
    <div class="group end">
      <button
        class:on={app.bottomOpen}
        use:tooltip={problems ? `Panel: ${errors} errors, ${problems - errors} warnings` : 'Panel (Ctrl+J)'}
        aria-label="Toggle bottom panel"
        aria-pressed={app.bottomOpen}
        onclick={toggleBottom}
      >
        <Icon name="panel" size={18} />
      </button>
      <button
        class:on={app.bottomOpen && app.bottomTab === 'problems'}
        use:tooltip={'Problems'}
        aria-label={problems ? `Problems, ${problems}` : 'Problems'}
        onclick={showProblems}
      >
        <Icon name={errors ? 'error' : 'warning'} size={18} />
        {#if problems > 0}<span class="badge" class:bad={errors > 0} class:warn={errors === 0}>{count(problems)}</span>{/if}
      </button>
    </div>
  {/if}
</nav>

<style>
  .rail {
    width: 42px;
    flex: none;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    padding: var(--sp-2) 0;
    background: var(--panel);
    border-right: 1px solid var(--line);
  }
  .group {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
  }
  button {
    position: relative;
    width: 42px;
    height: 36px;
    border: none;
    border-radius: 0;
    background: transparent;
    color: var(--dim);
    padding: 0;
    display: grid;
    place-items: center;
  }
  button:hover:not(:disabled) {
    color: var(--text);
    background: var(--hover);
    border-color: transparent;
  }
  button.on {
    color: var(--text);
  }
  button.on::before {
    content: '';
    position: absolute;
    left: 0;
    top: 8px;
    bottom: 8px;
    width: 3px;
    border-radius: 0 3px 3px 0;
    background: var(--accent);
  }
  .badge {
    position: absolute;
    right: 3px;
    bottom: 3px;
    min-width: 15px;
    padding: 0 3px;
    font-size: var(--fs-xs);
    line-height: 13px;
    border: 1px solid var(--panel);
  }
  .badge.warn {
    background: var(--warning);
    color: var(--on-accent);
  }
</style>
