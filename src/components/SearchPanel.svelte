<script lang="ts">
  import FilterInput from './FilterInput.svelte'
  import { ROW } from '../lib/view'
  import { api, errorText, readFileText } from '../lib/api'
  import { saveBuffer } from '../lib/buffers'
  import { openContextMenu, placeItems } from '../lib/context.svelte'
  import { ask } from '../lib/dialog.svelte'
  import { previewLine, replaceLines, type ReplaceOpts } from '../lib/editor/replace'
  import { applyReplace } from '../lib/edit.svelte'
  import { app, goTo } from '../lib/store.svelte'
  import type { RefHit, SearchHit } from '../lib/types'
  import VirtualList from './VirtualList.svelte'

  let query = $state('')
  let replacement = $state('')
  let replaceOpen = $state(false)
  let replacing = $state(false)
  let dialogueOnly = $state(false)
  let matchCase = $state(false)
  let wholeWord = $state(false)
  let useRegex = $state(false)
  let searching = $state(false)
  let hits = $state<SearchHit[]>([])
  let truncated = $state(false)
  let refs = $state<RefHit[]>([])
  let refTitle = $state('')
  let err = $state('')
  let timer: ReturnType<typeof setTimeout> | null = null
  let seenRef = 0
  let seenSearch = 0

  $effect(() => {
    const req = app.refRequest
    if (!req || req.seq === seenRef) return
    seenRef = req.seq
    refTitle = `${req.kind} ${req.name}`
    void api.findReferences(req.kind, req.name).then((rows) => {
      refs = rows
    }).catch((e) => {
      err = errorText(e)
    })
  })

  $effect(() => {
    const req = app.searchRequest
    if (!req || req.seq === seenSearch) return
    seenSearch = req.seq
    query = req.query
    dialogueOnly = req.dialogueOnly
    refs = []
    refTitle = ''
    void run()
  })

  function schedule() {
    if (timer) clearTimeout(timer)
    timer = setTimeout(() => void run(), 220)
  }

  function seedFor(q: string): string {
    if (!useRegex) return q
    return q.match(/[A-Za-z0-9_]{2,}/)?.[0] ?? ''
  }

  const shown = $derived.by(() => {
    let list = hits
    const q = query.trim()
    if (!q) return list
    if (useRegex) {
      try {
        const re = new RegExp(q, matchCase ? '' : 'i')
        list = list.filter((hit) => re.test(hit.text) || re.test(hit.path))
      } catch {
        return []
      }
    } else if (matchCase) {
      list = list.filter((hit) => hit.text.includes(q) || hit.path.includes(q))
    }
    if (wholeWord && !useRegex) {
      const escaped = q.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
      const re = new RegExp(`\\b${escaped}\\b`, matchCase ? '' : 'i')
      list = list.filter((hit) => re.test(hit.text))
    }
    return list
  })

  type Row = { kind: 'file'; path: string; count: number } | { kind: 'hit'; hit: SearchHit }

  const replaceOpts = $derived<ReplaceOpts>({
    query: query.trim(),
    replacement,
    matchCase,
    wholeWord,
    useRegex,
  })

  const rows = $derived.by(() => {
    const groups = new Map<string, SearchHit[]>()
    for (const hit of shown) {
      const list = groups.get(hit.path) ?? []
      list.push(hit)
      groups.set(hit.path, list)
    }
    const out: Row[] = []
    for (const [path, list] of groups) {
      out.push({ kind: 'file', path, count: list.length })
      for (const hit of list) out.push({ kind: 'hit', hit })
    }
    return out
  })

  async function run() {
    const q = query.trim()
    refs = []
    refTitle = ''
    if (q.length < 2 && !useRegex) {
      hits = []
      truncated = false
      searching = false
      return
    }
    const seed = seedFor(q)
    if (useRegex && seed.length < 2) {
      hits = []
      truncated = false
      err = 'A regex search still needs 2 letters or digits in the pattern. Those find candidate lines, then the pattern filters them.'
      return
    }
    searching = true
    try {
      const report = await api.searchProject(useRegex ? seed : q, dialogueOnly)
      hits = report.hits
      truncated = report.truncated
      err = ''
    } catch (e) {
      err = errorText(e)
    } finally {
      searching = false
    }
  }

  async function replaceIn(paths: string[]) {
    if (replacing || !paths.length) return
    if (truncated) {
      err = 'More than 200 matches. Narrow the search before replacing.'
      return
    }
    const opts = replaceOpts
    if (!opts.query || (opts.query.length < 2 && !opts.useRegex)) return
    if (replaceLines('x', [1], opts) == null) {
      err = 'That regular expression is not valid.'
      return
    }
    const dirty = paths.filter((p) => app.dirtyFiles.includes(p))
    if (dirty.length) {
      const saved = await ask({
        title: 'Save unsaved files before replacing?',
        note: dirty.join(', '),
        ok: 'Save',
        fields: [],
      })
      if (!saved) return
      for (const path of dirty) {
        if (!(await saveBuffer(path))) return
      }
    }
    replacing = true
    err = ''
    try {
      const changes: { path: string; before: string; text: string }[] = []
      for (const path of paths) {
        const lines = shown.filter((h) => h.path === path).map((h) => h.line)
        const before = await readFileText(path)
        const text = replaceLines(before, lines, opts)
        if (text == null) {
          err = 'That regular expression is not valid.'
          return
        }
        if (text !== before) changes.push({ path, before, text })
      }
      await applyReplace(changes)
      if (!app.error) await run()
    } catch (e) {
      err = errorText(e)
    } finally {
      replacing = false
    }
  }
</script>

<div class="sb-panel">
  <div class="sb-head"><h2>Search</h2></div>
  <div class="sb-tools">
    <FilterInput placeholder="Find in scripts…" bind:value={query} oninput={schedule} />
    <div class="replace-row">
      <button class="mini" type="button" aria-expanded={replaceOpen} onclick={() => { replaceOpen = !replaceOpen }}>{replaceOpen ? 'Hide replace' : 'Replace'}</button>
      {#if replaceOpen}
        <input type="text" placeholder="Replace with…" aria-label="Replace with" bind:value={replacement} />
        <button class="mini" type="button" disabled={replacing || truncated || shown.length === 0} onclick={() => void replaceIn([...new Set(shown.map((h) => h.path))])}>
          Replace all
        </button>
      {/if}
    </div>
    <div class="sb-chips" role="group" aria-label="Search options">
      <button type="button" class="sb-chip" class:on={dialogueOnly} aria-pressed={dialogueOnly} title="Only search dialogue and menu choices" onclick={() => { dialogueOnly = !dialogueOnly; schedule() }}>Dialogue</button>
      <button type="button" class="sb-chip" class:on={matchCase} aria-pressed={matchCase} title="Match case" onclick={() => { matchCase = !matchCase; schedule() }}>Aa</button>
      <button type="button" class="sb-chip" class:on={wholeWord} aria-pressed={wholeWord} title="Whole word" onclick={() => { wholeWord = !wholeWord; schedule() }}>Whole word</button>
      <button type="button" class="sb-chip" class:on={useRegex} aria-pressed={useRegex} title="Regular expression" onclick={() => { useRegex = !useRegex; schedule() }}>.*</button>
    </div>
  </div>
  {#if err}<div class="sb-foot">{err}</div>{/if}
  {#if refTitle}
    <div class="sb-foot">{refTitle} · {refs.length} references <button class="sb-link" onclick={() => { refs = []; refTitle = '' }}>clear</button></div>
    <div class="sb-list refs virtual">
      <VirtualList items={refs} rowHeight={ROW.lg}>
        {#snippet row(r)}
          <button class="sb-item" onclick={() => goTo(r.path, r.line)} oncontextmenu={(e) => openContextMenu(e, placeItems(r.path, r.line, r.text))}>
            <span class="sb-name wrap">{r.text}</span>
            <span class="sb-meta">{r.path}:{r.line}</span>
          </button>
        {/snippet}
      </VirtualList>
    </div>
  {/if}
  {#if !refTitle}
  <div class="sb-list virtual">
    {#if searching}
      <div class="sb-empty">Searching…</div>
    {:else if query.trim().length === 1 && !useRegex}
      <div class="sb-empty">Type at least two characters.</div>
    {:else if query.trim().length >= 2 && shown.length === 0 && !err}
      <div class="sb-empty">No matches.</div>
    {:else}
    <VirtualList items={rows} rowHeight={replaceOpen ? ROW.xl : ROW.md} itemKey={(r) => (r.kind === 'file' ? `f:${r.path}` : `${r.hit.path}:${r.hit.line}:${r.hit.text}`)}>
      {#snippet row(r)}
        {#if r.kind === 'file'}
          <div class="sb-group">
            <span class="sb-name">{r.path}</span>
            <em class="sb-tag">{r.count}</em>
            {#if replaceOpen}
              <button class="mini" type="button" disabled={replacing || truncated} onclick={() => void replaceIn([r.path])}>Replace</button>
            {/if}
          </div>
        {:else}
          {@const h = r.hit}
          {@const next = replaceOpen ? previewLine(h.text, replaceOpts) : null}
          <button
            class="sb-item"
            onclick={() => goTo(h.path, h.line)}
            title={`${h.path}:${h.line}`}
            oncontextmenu={(e) => openContextMenu(e, placeItems(h.path, h.line, h.speaker ? `${h.speaker}: ${h.text}` : h.text))}
          >
            <span class="sb-name">{#if h.speaker}{h.speaker}: {/if}{h.text}</span>
            {#if next}<span class="sb-meta preview">{next}</span>{/if}
            <span class="sb-meta">{h.kind} · line {h.line}</span>
          </button>
        {/if}
      {/snippet}
    </VirtualList>
    {/if}
  </div>
  {/if}
  {#if truncated && !refTitle}
    <div class="sb-foot">Showing the first 200 matches. Narrow the search before replacing.</div>
  {/if}
</div>

<style>
  .replace-row {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .replace-row input {
    flex: 1;
    min-width: 0;
  }
  .preview {
    color: var(--ok);
  }
</style>
