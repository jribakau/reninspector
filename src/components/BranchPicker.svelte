<script lang="ts">
  import { tick } from 'svelte'
  import { api, errorText } from '../lib/api'
  import { askText } from '../lib/dialog.svelte'
  import { git, loadGit } from '../lib/git.svelte'
  import { notify } from '../lib/toast.svelte'
  import { pushOverlay } from '../lib/overlay.svelte'
  import PaletteShell from './PaletteShell.svelte'

  let query = $state('')
  let picked = $state(0)
  let err = $state('')
  let box = $state<HTMLDivElement | null>(null)
  let input = $state<HTMLInputElement | null>(null)

  interface Row {
    id: string
    label: string
    hint: string
    run: () => void
  }

  const rows = $derived.by(() => {
    const q = query.trim().toLowerCase()
    const match = (name: string) => !q || name.toLowerCase().includes(q)
    const out: Row[] = [{ id: 'new', label: 'Create new branch…', hint: '', run: () => void create() }]
    for (const branch of git.branches) {
      if (branch.remote || !match(branch.name)) continue
      out.push({
        id: branch.name,
        label: branch.name,
        hint: branch.current ? 'current' : '',
        run: () => void checkout(branch.name, branch.current),
      })
    }
    for (const branch of git.branches) {
      if (!branch.remote || !match(branch.name)) continue
      out.push({
        id: `remote:${branch.name}`,
        label: branch.name,
        hint: 'remote',
        run: () => void checkout(branch.name, false),
      })
    }
    return out
  })

  function close() {
    git.picker = false
  }

  async function checkout(name: string, current: boolean) {
    if (current) {
      close()
      return
    }
    err = ''
    try {
      await api.gitSwitch(name)
      close()
      await loadGit()
      notify(`Switched to ${name}.`, 'ok')
    } catch (e) {
      err = errorText(e)
    }
  }

  async function create() {
    const name = await askText('Create branch', '', 'Create')
    if (!name) return
    err = ''
    try {
      await api.gitCreateBranch(name)
      close()
      await loadGit()
      notify(`Created ${name}.`, 'ok')
    } catch (e) {
      err = errorText(e)
    }
  }

  function choose(index: number) {
    const row = rows[index]
    if (row) row.run()
  }

  function key(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault()
      close()
    } else if (e.key === 'ArrowDown') {
      e.preventDefault()
      picked = Math.min(rows.length - 1, picked + 1)
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      picked = Math.max(0, picked - 1)
    } else if (e.key === 'Enter') {
      e.preventDefault()
      choose(picked)
    }
  }

  $effect(() => {
    if (!git.picker) return
    query = ''
    picked = 0
    err = ''
    const pop = pushOverlay({ kind: 'palette', el: () => box, onEscape: close })
    void tick().then(() => input?.focus())
    return pop
  })

  $effect(() => {
    if (!git.picker || picked < rows.length) return
    picked = 0
  })
</script>

{#if git.picker}
  <PaletteShell
    label="Branches"
    placeholder="Switch branch…"
    bind:query
    bind:box
    bind:input
    error={err}
    onclose={close}
    onkey={key}
    oninput={() => (picked = 0)}
  >
    <div class="pal-list">
      {#each rows as row, i (row.id)}
        <button class="pal-item" class:on={i === picked} onclick={() => choose(i)}>
          <span class="pal-lab">{row.label}</span>
          {#if row.hint}<span class="pal-key">{row.hint}</span>{/if}
        </button>
      {:else}
        <div class="pal-empty">No branches.</div>
      {/each}
    </div>
  </PaletteShell>
{/if}
