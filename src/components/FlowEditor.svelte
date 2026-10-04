<script lang="ts">
  import { onMount } from 'svelte'
  import { api } from '../lib/api'
  import {
    CHANNELS,
    POSITIONS,
    STAGE_CMDS,
    TRANSITIONS,
    beatLabel,
    emptyStage,
    insertSpot,
    spotLine,
    specFromStage,
    stageFromSpec,
    stageProblem,
    type EditorRequest,
    type InsertKind,
    type InsertSpot,
    type Place,
    type StageCmd,
  } from '../lib/flowedit'
  import { canRearrange, deleteLine, deleteNode, duplicateNode, moveNode } from '../lib/flowops.svelte'
  import { audioFiles, sceneEdit, sceneEditReport } from '../lib/scene.svelte'
  import type { GNode } from '../lib/types'

  interface Props {
    file: string
    request: EditorRequest
    canEdit: boolean
    characters: string[]
    scenes: string[]
    images: string[]
    transforms: string[]
    /** The scene this card points at, when it does not exist yet. */
    missingTarget: string
    style: string
    onclose: () => void
    onrequest: (r: EditorRequest) => void
    onopen: (scene: string) => void
    /** Create an empty scene with this name next to the others in the file. */
    oncreate: (scene: string) => Promise<boolean>
    ongoto: (file: string, line: number, endLine: number) => void
  }

  let {
    file,
    request,
    canEdit,
    characters,
    scenes,
    images,
    transforms,
    missingTarget,
    style,
    onclose,
    onrequest,
    onopen,
    oncreate,
    ongoto,
  }: Props = $props()

  let form: HTMLFormElement | undefined = $state()
  let err = $state('')
  let busy = $state(false)

  let speaker = $state('')
  let text = $state('')
  let target = $state('')
  let newScene = $state(false)
  let cond = $state('')
  let jumpKind = $state<'jump' | 'call'>('jump')
  let kind = $state<InsertKind>('say')
  let place = $state<Place>('after')
  let stage = $state(emptyStage())
  let audio = $state<string[]>([])
  let beatReady = $state(true)

  const node = $derived<GNode | null>(request.mode === 'insert' ? null : request.node)
  const speakers = $derived.by(() => {
    const names = [...characters]
    if (speaker && !names.includes(speaker)) names.unshift(speaker)
    return names
  })

  function init() {
    const r = request
    err = ''
    if (r.mode === 'insert') {
      kind = r.kind ?? 'say'
      place = r.spot.place
      speaker = r.speaker ?? ''
      text = ''
      target = ''
      newScene = false
      cond = ''
      stage = emptyStage('show')
      return
    }
    if (r.mode === 'beat') {
      beatReady = false
      stage = emptyStage()
      void api.sceneParse(`${r.beat.cmd} ${r.beat.text}`.trim()).then((spec) => {
        const f = spec ? stageFromSpec(spec) : null
        if (f) stage = f
        beatReady = !!f
      })
      return
    }
    const n = r.node
    speaker = n.speakers[0] ?? ''
    text = n.body || n.title
    target = n.target ?? ''
    newScene = false
    cond = r.cond
    jumpKind = n.kind === 'call' ? 'call' : 'jump'
  }
  init()

  onMount(() => {
    void audioFiles().then((list) => (audio = list))
    const el = form?.querySelector<HTMLElement>('textarea, input:not([type=checkbox]), select')
    el?.focus()
    if (el instanceof HTMLTextAreaElement || el instanceof HTMLInputElement) el.select?.()
  })

  const title = $derived.by(() => {
    const r = request
    if (r.mode === 'insert') return r.spot.place === 'before' ? 'Add above' : r.spot.place === 'into' ? 'Add inside' : 'Add below'
    if (r.mode === 'beat') return beatLabel(r.beat)
    const n = r.node
    switch (n.kind) {
      case 'choice':
        return 'Choice'
      case 'jump':
        return n.target ? `Jump to ${n.target}` : 'Jump'
      case 'call':
        return n.target ? `Call ${n.target}` : 'Call'
      case 'menu':
        return 'Menu'
      case 'return':
        return 'Return'
      default:
        return n.speakers[0] || 'Narrator'
    }
  })

  const isSay = $derived(request.mode === 'node' && request.node.kind === 'dialogue' && !!request.node.body)
  const isChoice = $derived(request.mode === 'node' && request.node.kind === 'choice')
  const isJump = $derived(request.mode === 'node' && (request.node.kind === 'jump' || request.node.kind === 'call'))
  const isMenu = $derived(request.mode === 'node' && request.node.kind === 'menu')
  const clipped = $derived(request.mode === 'node' && request.node.clipped)
  const hasJumpLine = $derived(isChoice && request.mode === 'node' && request.node.targetLine > 0)
  const writable = $derived(canEdit && !clipped)

  const anchorLine = $derived.by(() => (request.mode === 'insert' ? spotLine(request.spot, place) : 0))

  function setKind(k: InsertKind) {
    kind = k
    err = ''
  }

  function pickPlace(p: Place) {
    place = p
    if (p === 'before' && kind === 'choice') kind = 'say'
  }

  function stagePick(cmd: StageCmd) {
    const keep = { image: stage.image, at: stage.at, with: stage.with }
    stage = { ...emptyStage(cmd), ...keep }
  }

  async function apply() {
    if (!writable || busy) return
    err = ''
    const r = request
    busy = true
    try {
      if (r.mode === 'insert') {
        const done = await applyInsert(r.spot)
        if (done) onclose()
        return
      }
      if (r.mode === 'beat') {
        const problem = stageProblem(stage)
        if (problem) {
          err = problem
          return
        }
        if (await sceneEdit({ op: 'set-stmt', path: file, line: r.beat.line, spec: specFromStage(stage) })) onclose()
        return
      }
      const n = r.node
      if (isSay) {
        if (await sceneEdit({ op: 'set-say', path: file, line: n.line, speaker, text })) onclose()
      } else if (isChoice) {
        if (!text.trim()) {
          err = 'Write the choice first.'
          return
        }
        const next = target.trim()
        if (hasJumpLine && !next) {
          err = 'Name the scene this choice opens.'
          return
        }
        const ok = await sceneEdit({
          op: 'set-choice',
          path: file,
          line: n.line,
          text,
          target: hasJumpLine ? next : '',
          targetLine: n.targetLine,
          newLabel: newScene && hasJumpLine && !!next,
        })
        if (!ok) return
        if (cond.trim() !== r.cond.trim()) {
          if (!(await sceneEdit({ op: 'set-choice-cond', path: file, line: n.line, cond }))) return
        }
        onclose()
      } else if (isJump) {
        const next = target.trim()
        if (!next) {
          err = 'Name the scene.'
          return
        }
        const ok = await sceneEdit({
          op: 'set-stmt',
          path: file,
          line: n.line,
          spec: { kind: jumpKind, target: next },
          target: next,
          newLabel: newScene,
        })
        if (ok) onclose()
      }
    } finally {
      busy = false
    }
  }

  async function applyInsert(spot: InsertSpot): Promise<boolean> {
    const line = spotLine(spot, place)
    const base = { path: file, line, place }
    switch (kind) {
      case 'say': {
        if (!text.trim()) {
          err = 'Write the line first.'
          return false
        }
        return sceneEdit({ ...base, op: 'add-stmt', spec: { kind: 'say', speaker, text } })
      }
      case 'stage': {
        const problem = stageProblem(stage)
        if (problem) {
          err = problem
          return false
        }
        return sceneEdit({ ...base, op: 'add-stmt', spec: specFromStage(stage) })
      }
      case 'choice': {
        if (!text.trim() || !target.trim()) {
          err = text.trim() ? 'Name the scene this choice opens.' : 'Write the choice first.'
          return false
        }
        const report = await sceneEditReport({
          op: 'add-choice',
          path: file,
          line,
          text,
          target: target.trim(),
          newLabel: newScene,
        })
        if (!report) return false
        if (cond.trim() && report.focusLine > 0) {
          await sceneEdit({ op: 'set-choice-cond', path: file, line: report.focusLine, cond })
        }
        return true
      }
      case 'jump':
      case 'call': {
        const next = target.trim()
        if (!next) {
          err = 'Name the scene.'
          return false
        }
        return sceneEdit({ ...base, op: 'add-stmt', spec: { kind, target: next }, target: next, newLabel: newScene })
      }
      default:
        return sceneEdit({ ...base, op: 'add-stmt', spec: { kind: 'return' } })
    }
  }

  async function makeScene() {
    if (busy) return
    busy = true
    try {
      if (await oncreate(missingTarget)) onclose()
    } finally {
      busy = false
    }
  }

  function textKey(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
      e.preventDefault()
      form?.requestSubmit()
    }
  }

  function formKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault()
      e.stopPropagation()
      onclose()
    }
  }

  function addNear(side: 'above' | 'below') {
    const r = request
    if (r.mode === 'insert') return
    if (r.mode === 'beat') {
      const spot: InsertSpot = {
        line: r.beat.line,
        place: side === 'above' ? 'before' : 'after',
        places: ['before', 'after'],
      }
      onrequest({ mode: 'insert', spot })
      return
    }
    const spot = insertSpot(r.node, side)
    if (spot) onrequest({ mode: 'insert', spot })
  }

  const canAbove = $derived(request.mode === 'beat' || (request.mode === 'node' && !!insertSpot(request.node, 'above')))
  const canBelow = $derived(request.mode === 'beat' || (request.mode === 'node' && !!insertSpot(request.node, 'below')))
  const canMove = $derived(request.mode === 'beat' || (request.mode === 'node' && canRearrange(request.node)))
  const canCopy = $derived(request.mode === 'beat' || (request.mode === 'node' && canRearrange(request.node)))
  const showTools = $derived(request.mode !== 'insert' && canEdit)

  async function tool(run: () => Promise<boolean>) {
    if (busy) return
    busy = true
    try {
      if (await run()) onclose()
    } finally {
      busy = false
    }
  }

  const toolLine = $derived(request.mode === 'beat' ? request.beat.line : request.mode === 'node' ? request.node.line : 0)
  const toolEnd = $derived(request.mode === 'beat' ? request.beat.endLine : request.mode === 'node' ? request.node.endLine : 0)
</script>

{#snippet sceneButtons()}
  {#if canEdit && missingTarget && target.trim() === missingTarget}
    <button type="button" onclick={() => void makeScene()}>Create scene {missingTarget}</button>
  {:else if target.trim() && scenes.includes(target.trim())}
    <button type="button" onclick={() => onopen(target.trim())}>Open scene</button>
  {/if}
{/snippet}

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<form
  bind:this={form}
  class="fe"
  {style}
  onpointerdown={(e) => e.stopPropagation()}
  onwheel={(e) => e.stopPropagation()}
  onkeydown={formKey}
  onsubmit={(e) => {
    e.preventDefault()
    void apply()
  }}
>
  <header>
    <strong title={title}>{title}</strong>
    <button type="button" class="x" onclick={onclose} aria-label="Close">×</button>
  </header>

  {#if request.mode === 'insert'}
    {#if request.spot.places.length > 1}
      <div class="seg" role="group" aria-label="Where">
        {#each request.spot.places as p (p)}
          <button type="button" class:on={place === p} onclick={() => pickPlace(p)}>{p === 'before' ? 'Above' : p === 'after' ? 'Below' : 'Inside'}</button>
        {/each}
      </div>
    {/if}
    <div class="seg" role="group" aria-label="What to add">
      {#each [['say', 'Line'], ['stage', 'Staging'], ['choice', 'Choice'], ['jump', 'Jump'], ['call', 'Call'], ['return', 'Return']] as [id, label] (id)}
        <button type="button" class:on={kind === id} disabled={id === 'choice' && place === 'before'} onclick={() => setKind(id as InsertKind)}>{label}</button>
      {/each}
    </div>
  {/if}

  {#if clipped}
    <p class="hint">This line is too long to edit here.</p>
  {:else if (request.mode === 'insert' && kind === 'say') || isSay}
    <select bind:value={speaker} aria-label="Speaker">
      <option value="">narrator</option>
      {#each speakers as who (who)}
        <option value={who}>{who}</option>
      {/each}
    </select>
    <textarea bind:value={text} rows="2" aria-label="Dialogue" placeholder="What is said" onkeydown={textKey}></textarea>
  {:else if (request.mode === 'insert' && kind === 'stage') || request.mode === 'beat'}
    {#if request.mode === 'beat' && !beatReady}
      <p class="hint">The flow editor cannot change this line. Open it in code.</p>
    {:else}
      <select value={stage.cmd} aria-label="Command" onchange={(e) => stagePick(e.currentTarget.value as StageCmd)}>
        {#each STAGE_CMDS as c (c.id)}
          <option value={c.id}>{c.label}</option>
        {/each}
      </select>
      {#if stage.cmd === 'scene' || stage.cmd === 'show' || stage.cmd === 'hide'}
        <input bind:value={stage.image} list="fe-images" aria-label="Image" placeholder={stage.cmd === 'hide' ? 'Image tag' : 'Image'} />
      {/if}
      {#if stage.cmd === 'scene' || stage.cmd === 'show'}
        <input bind:value={stage.at} list="fe-positions" aria-label="Position" placeholder="At (left, right, ...)" />
      {/if}
      {#if stage.cmd === 'scene' || stage.cmd === 'show' || stage.cmd === 'hide' || stage.cmd === 'with'}
        <input bind:value={stage.with} list="fe-transitions" aria-label="Transition" placeholder={stage.cmd === 'with' ? 'Transition' : 'With (dissolve, fade, ...)'} />
      {/if}
      {#if stage.cmd === 'play' || stage.cmd === 'stop'}
        <select bind:value={stage.channel} aria-label="Channel">
          {#each CHANNELS as c (c)}
            <option value={c}>{c}</option>
          {/each}
        </select>
      {/if}
      {#if stage.cmd === 'play'}
        <input bind:value={stage.file} list="fe-audio" aria-label="Audio file" placeholder="audio/file.ogg" />
        <div class="pair">
          <input bind:value={stage.fadein} aria-label="Fade in seconds" placeholder="Fade in (s)" />
          <label><input type="checkbox" bind:checked={stage.looped} /> Loop</label>
        </div>
      {/if}
      {#if stage.cmd === 'stop'}
        <input bind:value={stage.fadeout} aria-label="Fade out seconds" placeholder="Fade out (s)" />
      {/if}
      {#if stage.cmd === 'pause'}
        <input bind:value={stage.secs} aria-label="Seconds" placeholder="Seconds (blank waits for a click)" />
      {/if}
    {/if}
  {:else if (request.mode === 'insert' && kind === 'choice') || isChoice}
    <input bind:value={text} aria-label="Choice" placeholder="Choice" />
    {#if request.mode === 'insert' || hasJumpLine}
      <input bind:value={target} list="fe-scenes" aria-label="Scene this choice opens" placeholder="Scene this choice opens" />
      <label><input type="checkbox" bind:checked={newScene} /> New scene</label>
      {@render sceneButtons()}
    {:else}
      <p class="hint">This choice runs several lines. Use Add below to write them.</p>
    {/if}
    <input bind:value={cond} aria-label="Only if" placeholder="Only if (a condition, optional)" />
  {:else if (request.mode === 'insert' && (kind === 'jump' || kind === 'call')) || isJump}
    {#if isJump}
      <div class="seg" role="group" aria-label="Kind">
        <button type="button" class:on={jumpKind === 'jump'} onclick={() => (jumpKind = 'jump')}>Jump</button>
        <button type="button" class:on={jumpKind === 'call'} onclick={() => (jumpKind = 'call')}>Call</button>
      </div>
    {/if}
    <input bind:value={target} list="fe-scenes" aria-label="Scene" placeholder="Scene" />
    <label><input type="checkbox" bind:checked={newScene} /> New scene</label>
    {@render sceneButtons()}
  {:else if request.mode === 'insert' && kind === 'return'}
    <p class="hint">Ends the scene and goes back to whoever called it.</p>
  {:else if isMenu}
    <p class="hint">A menu. Add a choice, or add a line next to it.</p>
    <button type="button" onclick={() => onrequest({ mode: 'insert', spot: { line: request.mode === 'node' ? request.node.line : 0, place: 'after', places: ['after'] }, kind: 'choice' })}>Add choice</button>
  {:else if request.mode === 'node' && request.node.kind === 'return'}
    <p class="hint">Ends the scene.</p>
  {/if}

  <datalist id="fe-scenes">
    {#each scenes as s (s)}<option value={s}></option>{/each}
  </datalist>
  <datalist id="fe-images">
    {#each images as s (s)}<option value={s}></option>{/each}
  </datalist>
  <datalist id="fe-positions">
    {#each [...POSITIONS, ...transforms.filter((t) => !POSITIONS.includes(t))] as s (s)}<option value={s}></option>{/each}
  </datalist>
  <datalist id="fe-transitions">
    {#each TRANSITIONS as s (s)}<option value={s}></option>{/each}
  </datalist>
  <datalist id="fe-audio">
    {#each audio as s (s)}<option value={s}></option>{/each}
  </datalist>

  {#if err}<p class="err">{err}</p>{/if}

  <div class="row">
    {#if writable && !isMenu && !(request.mode === 'node' && request.node.kind === 'return') && !(request.mode === 'beat' && !beatReady)}
      <button type="submit" class="primary" disabled={busy}>{request.mode === 'insert' ? 'Add' : 'Apply'}</button>
    {/if}
    {#if request.mode === 'insert' && anchorLine}
      <button type="button" onclick={() => ongoto(file, anchorLine, anchorLine)}>Code</button>
    {/if}
  </div>

  {#if showTools}
    <div class="tools" role="group" aria-label="Line actions">
      <button type="button" disabled={busy || !canAbove} onclick={() => addNear('above')}>Add above</button>
      <button type="button" disabled={busy || !canBelow} onclick={() => addNear('below')}>Add below</button>
      <button
        type="button"
        disabled={busy || !canMove}
        onclick={() => tool(() => (request.mode === 'beat' ? sceneEdit({ op: 'move-stmt', path: file, line: request.beat.line, dir: -1 }) : moveNode(file, node!, -1)))}
      >Up</button>
      <button
        type="button"
        disabled={busy || !canMove}
        onclick={() => tool(() => (request.mode === 'beat' ? sceneEdit({ op: 'move-stmt', path: file, line: request.beat.line, dir: 1 }) : moveNode(file, node!, 1)))}
      >Down</button>
      <button
        type="button"
        disabled={busy || !canCopy}
        onclick={() => tool(() => (request.mode === 'beat' ? sceneEdit({ op: 'duplicate-stmt', path: file, line: request.beat.line }) : duplicateNode(file, node!)))}
      >Copy</button>
      <button
        type="button"
        class="danger"
        disabled={busy || !canCopy}
        onclick={() => tool(() => (request.mode === 'beat' ? deleteLine(file, request.beat.line) : deleteNode(file, node!)))}
      >Delete</button>
      <button type="button" onclick={() => ongoto(file, toolLine, toolEnd)}>Code</button>
    </div>
  {/if}
  {#if !canEdit}
    <p class="hint">This file is read-only. Open it in code to look at the source.</p>
  {/if}
</form>

<style>
  .fe {
    position: absolute;
    z-index: 4;
    width: 320px;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px;
    background: var(--panel);
    border: 1px solid var(--accent);
    border-radius: var(--r-lg);
-shadow: var(--shadow-lg);
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
  }
  header strong {
    font-size: var(--fs-md);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .x {
    padding: 0 7px;
    line-height: 18px;
  }
  .fe input,
  .fe select,
  .fe textarea {
    width: 100%;
    box-sizing: border-box;
  }
  .fe input[type='checkbox'] {
    width: auto;
  }
  textarea {
    resize: none;
    field-sizing: content;
    min-height: 2.6em;
    max-height: 170px;
    font: inherit;
    padding: 5px 8px;
    color: var(--text);
    background: var(--panel-2);
    border: 1px solid var(--line);
    border-radius: var(--r-md);
  }
  textarea:focus {
    outline: 1px solid var(--accent);
  }
  label,
  .hint {
    margin: 0;
    color: var(--dim);
    font-size: var(--fs-md);
  }
  .err {
    margin: 0;
    color: var(--error);
    font-size: var(--fs-md);
  }
  .row,
  .pair {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .seg {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .seg button {
    font-size: var(--fs-sm);
    padding: 2px 8px;
  }
  .seg button.on {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 22%, var(--panel-2));
  }
  .tools {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    padding-top: 6px;
    border-top: 1px solid var(--line);
  }
  .tools button {
    font-size: var(--fs-sm);
    padding: 2px 8px;
  }
  .tools button.danger:hover:not(:disabled) {
    border-color: var(--error);
    color: var(--error);
  }
</style>
