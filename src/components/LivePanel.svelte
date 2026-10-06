<script lang="ts">
  import { onMount } from 'svelte'
  import Icon from './Icon.svelte'
  import { copyText, labelItems, openContextMenu } from '../lib/context.svelte'
  import { resumeLive, stepLive } from '../lib/breaks.svelte'
  import { app, goTo, jumpGameHere, jumpLiveLabel, reloadLive, setWatchVars, toggleFollowGame, toggleLive } from '../lib/store.svelte'

  let name = $state('')
  let jumpName = $state('')
  let now = $state(Date.now())

  const age = $derived(
    app.live.receivedAt && app.live.running ? Math.max(0, Math.round((now - app.live.receivedAt) / 1000)) : null,
  )

  const warpOff = $derived(app.live.running && !app.live.canWarp)

  const labelHints = $derived.by(() => {
    const q = jumpName.trim().toLowerCase()
    if (!q) return [] as string[]
    const seen = new Set<string>()
    const out: string[] = []
    for (const s of app.catalog?.symbols ?? []) {
      if (s.kind !== 'label' || seen.has(s.name) || !s.name.toLowerCase().includes(q)) continue
      seen.add(s.name)
      out.push(s.name)
      if (out.length >= 8) break
    }
    return out
  })

  const suggestions = $derived(
    (app.catalog?.variables ?? [])
      .map((v) => v.name)
      .filter((n) => !app.watchVars.includes(n))
      .slice(0, 80),
  )

  const values = $derived(Object.entries(app.live.vars))

  onMount(() => {
    const timer = setInterval(() => (now = Date.now()), 1000)
    return () => clearInterval(timer)
  })

  function add() {
    const next = name.trim()
    name = ''
    if (!next || app.watchVars.includes(next)) return
    void setWatchVars([...app.watchVars, next])
  }

  function remove(n: string) {
    void setWatchVars(app.watchVars.filter((v) => v !== n))
  }

  function show() {
    if (app.live.file && app.live.line) goTo(app.live.file, app.live.line)
  }

  function jumpLabel(next = jumpName) {
    const name = next.trim()
    if (!name) return
    jumpName = name
    void jumpLiveLabel(name)
  }
</script>

<div class="sb-panel live">
  <div class="sb-tools">
    {#if app.live.running}
      <div class="sb-meta">
        Game is running
        {#if age !== null}· heard {age < 2 ? 'just now' : `${age}s ago`}{/if}
      </div>
    {:else}
      <button class="primary" onclick={() => void toggleLive()} disabled={!!app.busy}>Start live</button>
      <div class="sb-meta">Runs the game in its own window and follows the line it is on. F5 starts and stops it.</div>
    {/if}
    <label class="check">
      <input type="checkbox" checked={app.followGame} onchange={toggleFollowGame} />
      <span>Follow the game in the editor</span>
    </label>
    {#if app.live.running}
      <div class="row">
        <button class="sb-item" onclick={show} disabled={!app.live.file}>
          <span class="sb-name">{app.live.label || 'No label yet'}</span>
          <span class="sb-meta">{app.live.file ? `${app.live.file}:${app.live.line}` : 'Waiting for the first line…'}</span>
        </button>
        <button onclick={toggleLive} title="Stop the live session (F5)">Stop</button>
      </div>
      {#if app.live.warpNotes.length}
        <details>
          <summary class="sb-meta">Warp notes</summary>
          {#each app.live.warpNotes as line}
            <div class="sb-meta">{line}</div>
          {/each}
        </details>
      {/if}
      {#if app.live.speaker}
        <div class="sb-meta">Speaker: {app.live.speaker}</div>
      {/if}
      {#if app.live.showing.length}
        <div class="sb-meta">On screen: {app.live.showing.join(', ')}</div>
      {/if}
      {#if app.live.paused}
        <div class="sb-meta">Paused before this statement. Step runs it and stops at the next one.</div>
        <div class="row">
          <button onclick={() => void stepLive()}>Step</button>
          <button class="primary" onclick={() => void resumeLive()}>Resume</button>
        </div>
      {:else}
        <button onclick={() => void stepLive()} title="Pause before the next statement">Step</button>
      {/if}
      {#if app.live.stack.length}
        <details open={app.live.paused}>
          <summary class="sb-meta">Call stack</summary>
          {#each app.live.stack as frame, i (`${frame.file}:${frame.line}:${i}`)}
            <button class="sb-item" onclick={() => frame.file && goTo(frame.file, frame.line)} disabled={!frame.file}>
              <span class="sb-name">{frame.label || 'statement'}</span>
              <span class="sb-meta">{frame.file ? `${frame.file}:${frame.line}` : ''}</span>
            </button>
          {/each}
        </details>
      {/if}
      {#if app.live.canReload}
        <button onclick={reloadLive} title="Reload scripts in the running game">Reload scripts</button>
      {/if}
      <form class="row" onsubmit={(e) => { e.preventDefault(); jumpLabel() }}>
        <input placeholder="Jump to label" bind:value={jumpName} />
        <button type="submit" disabled={!jumpName.trim()} title="Jump the running game to this label">Jump</button>
      </form>
      {#each labelHints as hint (hint)}
        <button
          class="sb-item"
          onclick={() => jumpLabel(hint)}
          oncontextmenu={(e) =>
            openContextMenu(e, [{ kind: 'item', label: 'Jump', run: () => jumpLabel(hint) }, ...labelItems(hint)])}
        >
          <span class="sb-name">{hint}</span>
        </button>
      {/each}
      {#if app.live.replay === 'pending'}
        <div class="sb-meta">Replay is waiting for the main menu, then it runs from the start.</div>
      {:else if app.live.replay === 'running'}
        <div class="sb-meta">Replaying from the start toward the chosen line.</div>
      {:else if app.live.replay === 'done'}
        <div class="sb-meta">Replay reached the line. The game is waiting for you.</div>
      {:else if app.live.replay === 'diverged'}
        <div class="sb-meta">Replay stopped: {app.live.replayReason || 'the story left the planned path.'}</div>
      {:else if app.live.replay === 'stalled'}
        <div class="sb-meta">{app.live.replayReason || 'Replay is waiting. A screen or prompt may need you.'}</div>
      {/if}
      {#if app.replayAssumptions.length}
        <details>
          <summary class="sb-meta">Assumed conditions</summary>
          {#each app.replayAssumptions as cond}
            <div class="sb-meta">{cond}</div>
          {/each}
        </details>
      {/if}
    {/if}
    {#if app.replayBlocked}
      <button onclick={() => jumpGameHere()} disabled={warpOff} title="Open the game at the caret. Earlier dialogue is skipped.">Run from here instead</button>
      {#if warpOff}
        <div class="sb-meta">Run from here is off. This session cannot warp to a line.</div>
      {/if}
    {/if}
    {#if app.live.replay || warpOff}
      <div class="sb-meta">Replay runs the story from the start. Run from here does not, so a variable can still be wrong.</div>
    {/if}
  </div>

  <div class="sb-tools">
    <div class="sb-meta">Watched variables</div>
    <form class="row" onsubmit={(e) => { e.preventDefault(); add() }}>
      <input list="vnide-vars" placeholder="Variable name" bind:value={name} />
      <button type="submit">Add</button>
    </form>
    <datalist id="vnide-vars">
      {#each suggestions as s (s)}
        <option value={s}></option>
      {/each}
    </datalist>
  </div>

  <div class="sb-list">
    {#each app.watchVars as n (n)}
      {@const shown = values.find(([k]) => k === n)}
      <div class="sb-item row" role="group" oncontextmenu={(e) => openContextMenu(e, [{ kind: 'item', label: 'Copy name', run: () => copyText(n) }])}>
        <span class="sb-name">{n}</span>
        <span class="sb-meta" title={shown ? String(shown[1]) : app.live.running ? 'Not reported yet' : 'Live is stopped'}>{shown ? String(shown[1]) : app.live.running ? '…' : '—'}</span>
        <button class="icon sm" onclick={() => remove(n)} title="Stop watching" aria-label={`Stop watching ${n}`}>
          <Icon name="close" size={12} />
        </button>
      </div>
    {:else}
      <div class="sb-empty">Add a name from the Vars list. The game reports it on each click.</div>
    {/each}
  </div>
</div>

<style>
  /* The tools stay their natural height. The panel scrolls instead of painting over the status bar. */
  .live {
    overflow: auto;
  }
  .live :global(.sb-tools) {
    min-width: 0;
  }
  .live :global(.sb-tools > *) {
    min-width: 0;
  }
  .live :global(.sb-list) {
    flex: 1 0 auto;
    min-height: auto;
    overflow: visible;
  }
  .row {
    display: flex;
    gap: 6px;
    align-items: center;
    min-width: 0;
  }
  .row > .sb-item {
    flex: 1;
    min-width: 0;
    width: auto;
    height: auto;
  }
  .row > input {
    flex: 1;
    min-width: 0;
    width: auto;
  }
  .check {
    display: flex;
    gap: 6px;
    align-items: center;
    min-width: 0;
    font-size: var(--fs-md);
  }
  .check input {
    flex: none;
    width: auto;
    padding: 0;
  }
  .check span {
    min-width: 0;
  }
</style>
