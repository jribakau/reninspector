<script lang="ts">
  import EmptyState from './EmptyState.svelte'
  import Icon from './Icon.svelte'
  import FilterInput from './FilterInput.svelte'
  import { ROW } from '../lib/view'
  import { copyText, openContextMenu } from '../lib/context.svelte'
  import { problemUi, visibleProblems } from '../lib/problems.svelte'
  import { app, goTo, revealInExplorer, selectLabel } from '../lib/store.svelte'
  import type { Diagnostic, Severity } from '../lib/types'
  import VirtualList from './VirtualList.svelte'

  let collapsed = $state<Set<string>>(new Set())

  const problems = $derived(visibleProblems())

  type Row = { kind: 'file'; path: string; count: number } | { kind: 'diag'; d: Diagnostic }

  const rows = $derived.by(() => {
    const groups = new Map<string, Diagnostic[]>()
    for (const d of problems) {
      const list = groups.get(d.path) ?? []
      list.push(d)
      groups.set(d.path, list)
    }
    const out: Row[] = []
    for (const [path, list] of groups) {
      const key = path || '(no file)'
      out.push({ kind: 'file', path: key, count: list.length })
      if (!collapsed.has(key)) for (const d of list) out.push({ kind: 'diag', d })
    }
    return out
  })

  $effect(() => {
    const cursor = app.problemCursor
    if (!cursor) return
    const key = cursor.path || '(no file)'
    if (!collapsed.has(key)) return
    const next = new Set(collapsed)
    next.delete(key)
    collapsed = next
  })

  const reveal = $derived.by(() => {
    const cursor = app.problemCursor
    if (!cursor) return null
    const index = rows.findIndex((row) => row.kind === 'diag' && row.d.path === cursor.path && row.d.line === cursor.line)
    return index >= 0 ? index : null
  })

  function toggleFile(path: string) {
    const next = new Set(collapsed)
    if (next.has(path)) next.delete(path)
    else next.add(path)
    collapsed = next
  }
  const hiddenInfos = $derived(!problemUi.info && (app.diag?.infos ?? 0) > 0)

  function openProblem(d: Diagnostic) {
    if (d.path) goTo(d.path, d.line)
    if (d.label) selectLabel(d.label, { reveal: false })
  }

  function sevCount(s: Severity): number {
    const d = app.diag
    if (!d) return 0
    return s === 'error' ? d.errors : s === 'warning' ? d.warnings : d.infos
  }

  function sevOn(s: Severity): boolean {
    return s === 'error' ? problemUi.error : s === 'warning' ? problemUi.warning : problemUi.info
  }
</script>

<div class="sb-panel">
  <div class="sb-tools">
    <FilterInput placeholder="Filter problems…" bind:value={problemUi.filter} />
    <div class="sb-chips">
      {#each ['error', 'warning', 'info'] as s (s)}
        <button class="sb-chip" class:on={sevOn(s as Severity)} onclick={() => {
          const key = s as Severity
          if (key === 'error') problemUi.error = !problemUi.error
          else if (key === 'warning') problemUi.warning = !problemUi.warning
          else problemUi.info = !problemUi.info
        }}>
          <span class={`sev ${s}`}><Icon name={s === 'info' ? 'info' : (s as 'error' | 'warning')} size={12} /></span>{s} {sevCount(s as Severity)}
        </button>
      {/each}
    </div>
  </div>
  <div class="sb-list virtual">
    {#if rows.length}
      <VirtualList items={rows} rowHeight={ROW.sm} reveal={reveal} revealSeq={app.problemCursor?.seq ?? 0} itemKey={(row) => row.kind === 'file' ? `f:${row.path}` : `${row.d.path}:${row.d.line}:${row.d.code}:${row.d.message}`}>
        {#snippet row(row, index)}
          {#if row.kind === 'file'}
            <button class="sb-item file row" aria-expanded={!collapsed.has(row.path)} onclick={() => toggleFile(row.path)}>
              <span class="twist" class:open={!collapsed.has(row.path)}><Icon name="chevron-right" size={12} /></span>
              <span class="sb-name">{row.path} <em class="sb-tag">{row.count}</em></span>
            </button>
          {:else}
            {@const d = row.d}
            <button
              class="sb-item"
              class:sel={index === reveal}
              onclick={() => openProblem(d)}
              title={`${d.message} (${d.path}:${d.line})`}
              oncontextmenu={(e) =>
                openContextMenu(e, [
                  { kind: 'item', label: 'Open', enabled: !!d.path, run: () => openProblem(d) },
                  { kind: 'item', label: 'Copy', run: () => copyText(`${d.message} (${d.path}:${d.line})`) },
                  { kind: 'item', label: 'Reveal in explorer', enabled: !!d.path, run: () => d.path && revealInExplorer(d.path) },
                ])}
            >
              <span class="sb-name sb-line"><span class={`sev ${d.severity}`}><Icon name={d.severity === 'info' ? 'info' : d.severity} size={13} /></span>{d.message}</span>
            </button>
          {/if}
        {/snippet}
      </VirtualList>
    {:else if !(app.diag?.items.length)}
      <EmptyState tone="ok" icon="check" title="No problems" hint="Errors and warnings from the project show up here." />
    {:else}
      <EmptyState
        icon="filter"
        title="No problems match"
        hint={hiddenInfos ? 'Info messages are hidden.' : 'Try a different filter.'}
        action="Show all"
        onaction={() => {
          problemUi.filter = ''
          problemUi.error = true
          problemUi.warning = true
          problemUi.info = true
        }}
      />
    {/if}
  </div>
  {#if (app.diag?.truncated ?? 0) > 0}
    <div class="sb-foot">{app.diag?.truncated} more problems not shown.</div>
  {/if}
</div>

<style>
  .sev {
    display: inline-flex;
    vertical-align: middle;
    margin-right: 6px;
    color: var(--dim);
  }
  .sev.error {
    color: var(--error);
  }
  .sev.warning {
    color: var(--warning);
  }
  .sb-line {
    display: flex;
    align-items: center;
  }
</style>
