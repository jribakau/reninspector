<script lang="ts">
  import { openContextMenu } from '../lib/context.svelte'
  import { app, goTo, revealInExplorer } from '../lib/store.svelte'
  import { isStageLiteral, resetStagePins, setStagePin, stageShowsLive, stageSummary, stageUi, stageVars, unlistedPins } from '../lib/stage.svelte'
  import { setSetting, settings } from '../lib/settings.svelte'
  import { ensureTrusted } from '../lib/trust.svelte'
  import type { StagePicture, StageSprite } from '../lib/types'

  const animate = $derived(settings.stageAnimate)
  let clock = $state(0)

  function toggleAnimate() {
    setSetting('stageAnimate', !animate)
  }

  /** The picture shown `t` seconds in. A frame without a pause lasts 0.1 s. */
  function frameAt(picture: StagePicture, t: number): StagePicture {
    if (picture.kind !== 'frames') return picture
    const { frames } = picture
    if (!frames.length) return { kind: 'unknown' }
    const lengths = frames.map((f) => (f.seconds > 0 ? f.seconds : 0.1))
    const total = lengths.reduce((a, b) => a + b, 0)
    let at = picture.repeat ? t % total : Math.min(t, total - 0.001)
    for (let i = 0; i < frames.length; i++) {
      if (at < lengths[i]) return frames[i].picture
      at -= lengths[i]
    }
    return frames[frames.length - 1].picture
  }

  /** The sprite as it looks now. */
  function drawn(sprite: StageSprite): StageSprite {
    return sprite.picture.kind === 'frames' ? { ...sprite, picture: frameAt(sprite.picture, animate ? clock : 0) } : sprite
  }

  /** The sprite with its first frame, which sizes the box so it doesn't jump. */
  function base(sprite: StageSprite): StageSprite {
    return sprite.picture.kind === 'frames' ? { ...sprite, picture: frameAt(sprite.picture, 0) } : sprite
  }

  function tipOf(sprite: StageSprite): string {
    return sprite.picture.kind === 'frames' ? `${sprite.name} · ${sprite.picture.frames.length} frames` : sprite.name
  }

  let frameW = $state(0)
  let natural = $state<Record<string, { w: number; h: number }>>({})
  let shotAr = $state(0)
  let failed = $state<Record<string, boolean>>({})

  // `natural` is keyed by path and refilled by each image's onload, which also fires when a
  // path gets a new blob URL. Clearing it per estimate would leave already-loaded images
  // hidden, since they never load again. Only the failure marks need to reset.
  $effect(() => {
    void stageUi.images
    failed = {}
  })

  // The same relative path in another project is a different file.
  $effect(() => {
    void app.info?.root
    natural = {}
    shotAr = 0
  })

  const estimate = $derived(stageUi.estimate)
  const pinCount = $derived(Object.keys(stageVars.overrides).length)
  const extraPins = $derived(unlistedPins(stageVars.overrides, estimate?.vars.map((v) => v.name) ?? []))
  let condsOpen = $state(true)
  let condErrors = $state<Record<string, string>>({})
  /** Typed text that has not been saved yet, including a value that was rejected. */
  let drafts = $state<Record<string, string>>({})

  function fieldOf(name: string): string {
    if (name in drafts) return drafts[name]
    return stageVars.overrides[name] ?? ''
  }

  function commitPin(name: string, input: HTMLInputElement) {
    const text = input.value.trim()
    if (!text) {
      condErrors = { ...condErrors, [name]: '' }
      drafts = { ...drafts, [name]: '' }
      input.value = ''
      setStagePin(name, null)
      return
    }
    if (!isStageLiteral(text)) {
      const saved = stageVars.overrides[name] ?? ''
      condErrors = { ...condErrors, [name]: 'Use a number, True, False, None, or a quoted string.' }
      drafts = { ...drafts, [name]: saved }
      input.value = saved
      return
    }
    condErrors = { ...condErrors, [name]: '' }
    const next = { ...drafts }
    delete next[name]
    drafts = next
    setStagePin(name, text)
  }

  function clearPin(name: string) {
    condErrors = { ...condErrors, [name]: '' }
    drafts = { ...drafts, [name]: '' }
    setStagePin(name, null)
  }
  const live = $derived(stageShowsLive())
  const animated = $derived(!!estimate?.sprites.some((s) => s.picture.kind === 'frames'))

  $effect(() => {
    if (!animated || !animate || live) {
      clock = 0
      return
    }
    const start = performance.now()
    const id = setInterval(() => {
      clock = (performance.now() - start) / 1000
    }, 50)
    return () => clearInterval(id)
  })
  const scale = $derived(estimate && frameW > 0 ? frameW / estimate.width : 0)
  const notes = $derived.by(() => {
    const est = stageUi.estimate
    const extra = stageUi.imageNotes
    if (!est) return extra
    // The backend already says when a file is nowhere in the project; add only the other read failures.
    const reads = Object.entries(stageUi.failedReads)
      .filter(([path]) => !est.notes.some((n) => n.includes(`\`${path}\``)))
      .map(([path, why]) => `Could not read \`${path}\`: ${why}`)
    return [...est.assumptions, ...est.notes, ...reads, ...extra]
  })

  /** Why a flat placeholder is shown instead of a picture. */
  function reasonOf(sprite: StageSprite, path: string): string {
    if (path) {
      if (stageUi.failedReads[path] || failed[path]) return `Missing file: ${path}`
      return 'Loading…'
    }
    if (sprite.picture.kind === 'unknown') return "Can't draw this picture"
    return ''
  }

  function url(path: string | null | undefined): string | undefined {
    return path ? stageUi.images[path] : undefined
  }

  function fitInto(fit: string, w: number, h: number, boxW: number, boxH: number): [number, number] {
    if (fit === 'fill') return [boxW, boxH]
    const contain = Math.min(boxW / w, boxH / h)
    const s =
      fit === 'cover'
        ? Math.max(boxW / w, boxH / h)
        : fit === 'scale-down'
          ? Math.min(1, contain)
          : fit === 'scale-up'
            ? Math.max(1, contain)
            : contain
    return [w * s, h * s]
  }

  /** Drawn size in game pixels: the image's own Transform, then the `at` transform's fit, then zoom. */
  function drawnSize(sprite: StageSprite, natW: number, natH: number, real: boolean): [number, number] {
    if (!estimate) return [natW, natH]
    if (!real) return [natW, natH]
    let w = natW
    let h = natH
    const fit = sprite.fit ?? (sprite.sizeW != null || sprite.sizeH != null ? 'fill' : null)
    if (fit) {
      const area = fit !== 'fill'
      const boxW = sprite.sizeW ?? (area ? estimate.width : natW)
      const boxH = sprite.sizeH ?? (area ? estimate.height : natH)
      ;[w, h] = fitInto(fit, w, h, boxW, boxH)
    }
    if (sprite.placeFit) [w, h] = fitInto(sprite.placeFit, w, h, estimate.width, estimate.height)
    const zoom = sprite.zoom ?? 1
    return [w * zoom, h * zoom]
  }

  function placed(sprite: StageSprite): string | null {
    if (!estimate) return null
    if (sprite.picture.kind === 'color') return 'left:0;top:0;width:100%;height:100%'
    let natW: number
    let natH: number
    let real: boolean
    if (sprite.picture.kind === 'layers') {
      natW = sprite.picture.w
      natH = sprite.picture.h
      real = natW > 0 && natH > 0
    } else {
      const path = sprite.picture.kind === 'file' ? sprite.picture.path : ''
      const known = path ? natural[path] : undefined
      natW = known?.w ?? sprite.sizeW ?? estimate.width * 0.3
      natH = known?.h ?? sprite.sizeH ?? estimate.height * 0.6
      real = !!known
    }
    const [imgW, imgH] = drawnSize(sprite, natW, natH, real)
    const xpos = sprite.xpos ?? sprite.xalign ?? 0.5
    const ypos = sprite.ypos ?? sprite.yalign ?? 1
    const xanchor = sprite.xanchor ?? sprite.xalign ?? 0.5
    const yanchor = sprite.yanchor ?? sprite.yalign ?? 1
    const px = (xpos <= 1 ? xpos * estimate.width : xpos) - xanchor * imgW
    const py = (ypos <= 1 ? ypos * estimate.height : ypos) - yanchor * imgH
    const path = sprite.picture.kind === 'file' ? sprite.picture.path : ''
    const hidden = path && !natural[path] ? ';visibility:hidden' : ''
    return `left:${px}px;top:${py}px;width:${imgW}px;height:${imgH}px${hidden}`
  }

  /** How much a composite's own pixels are scaled to the box `placed` drew. */
  function layerScale(sprite: StageSprite): [number, number] {
    if (sprite.picture.kind !== 'layers' || sprite.picture.w <= 0 || sprite.picture.h <= 0) {
      const z = sprite.zoom ?? 1
      return [z, z]
    }
    const [imgW, imgH] = drawnSize(sprite, sprite.picture.w, sprite.picture.h, true)
    return [imgW / sprite.picture.w, imgH / sprite.picture.h]
  }

  function shotSize(event: Event) {
    const img = event.currentTarget
    if (!(img instanceof HTMLImageElement) || !img.naturalWidth) return
    shotAr = img.naturalWidth / img.naturalHeight
  }

  function miss(path: string) {
    if (!path || failed[path]) return
    failed = { ...failed, [path]: true }
  }

  function remember(path: string | null, event: Event) {
    const img = event.currentTarget
    if (!(img instanceof HTMLImageElement) || !img.naturalWidth || !path) return
    if (natural[path]?.w === img.naturalWidth && natural[path]?.h === img.naturalHeight) return
    natural = { ...natural, [path]: { w: img.naturalWidth, h: img.naturalHeight } }
  }

  function openShow(sprite: StageSprite) {
    if (sprite.shownFile) goTo(sprite.shownFile, sprite.shownLine)
  }

  function spriteMenu(e: MouseEvent, sprite: StageSprite) {
    const items: { kind: 'item'; label: string; run: () => void }[] = [
      { kind: 'item', label: 'Go to show', run: () => openShow(sprite) },
    ]
    if (sprite.defineFile && sprite.defineLine) {
      const file = sprite.defineFile
      const line = sprite.defineLine
      items.push({ kind: 'item', label: 'Open image definition', run: () => goTo(file, line) })
    } else if (sprite.picture.kind === 'file' && sprite.picture.path) {
      const path = sprite.picture.path
      items.push({ kind: 'item', label: 'Reveal image', run: () => revealInExplorer(path) })
    }
    openContextMenu(e, items)
  }

  const choiceH = $derived.by(() => {
    if (!estimate) return 0
    const gui = estimate.gui
    const known = gui.choice ? natural[gui.choice] : undefined
    return Math.max(known?.h ?? 0, gui.choiceTextSize * 1.4)
  })
</script>

<div class="stage">
  <div class="head pane-head" aria-live="polite">
    {#if live}
      <span class="mode live">Live</span>
      <button class="text" aria-pressed={stageUi.preferEstimate} onclick={() => (stageUi.preferEstimate = true)}>Show estimate</button>
    {:else if estimate}
      <span class="mode">{stageUi.pending ? 'Updating' : 'Estimate'}</span>
      <span class="dim">{stageSummary(estimate)}</span>
      {#if pinCount}
        <span class="dim">{pinCount} pinned</span>
        <button class="text" title="Clear the values pinned for this project's stage preview" onclick={resetStagePins}>
          Reset all
        </button>
      {/if}
      {#if stageUi.shot && stageUi.preferEstimate}
        <button class="text" aria-pressed="true" onclick={() => (stageUi.preferEstimate = false)}>Show live</button>
      {/if}
    {:else}
      <span class="mode">Stage</span>
    {/if}
    {#if animated && !live}
      <button class="text" aria-pressed={animate} title="Play the frames of animated images" onclick={toggleAnimate}>
        {animate ? 'Animating' : 'Animate'}
      </button>
    {/if}
    {#if stageUi.shot && !live}
      <button class="text" title="Move the caret to the live screenshot" onclick={() => stageUi.shot && goTo(stageUi.shot.file, stageUi.shot.line)}>
        Live at {stageUi.shot.file.split('/').pop()}:{stageUi.shot.line}
      </button>
    {/if}
    <span class="tail">
      {#if stageUi.error}<span class="bad" title={stageUi.error}>{stageUi.error}</span>{/if}
      {#if stageUi.shotError}<span class="bad" title={stageUi.shotError}>{stageUi.shotError}</span>{/if}
      {#if stageUi.imageStatus}<span class="dim status" title={stageUi.imageStatus}>{stageUi.imageStatus}</span>{/if}
      {#if stageUi.needsTrust}
        <button class="text" title="Start the game's engine in the background to read its image definitions. This runs the game's code." onclick={() => void ensureTrusted()}>
          Use engine images
        </button>
      {/if}
    </span>
  </div>
  {#if live && stageUi.shot}
    <div class="fit">
      <div class="frame shot" class:dim={stageUi.shotStale} style={`--ar:${shotAr || (estimate ? estimate.width / estimate.height : 1.777778)}`}>
        <img src={stageUi.shot.url} alt="The game's current screen" onload={shotSize} />
      </div>
    </div>
  {:else if estimate}
    {@const gui = estimate.gui}
    <div class="fit">
    <div class="frame" class:dim={!!stageUi.pending || !!stageUi.error} style={`--ar:${estimate.width / estimate.height}`} bind:clientWidth={frameW}>
      <div
        class="world"
        style={`width:${estimate.width}px;height:${estimate.height}px;transform:scale(${scale})`}
      >
        {#each estimate.sprites as source, i (`${source.layer}:${source.tag}`)}
          {@const sprite = drawn(source)}
          {@const tip = tipOf(source)}
          {@const box = placed(base(source))}
          {@const filePath = sprite.picture.kind === 'file' ? sprite.picture.path : ''}
          {#if box}
            {#if sprite.picture.kind === 'color'}
              <button
                class="layer color"
                style={`${box};background:${sprite.picture.hex};z-index:${i}`}
                aria-label={`Go to show: ${sprite.name}`}
                title={tip}
                onclick={() => openShow(sprite)}
                oncontextmenu={(e) => spriteMenu(e, sprite)}
              ></button>
            {:else if sprite.picture.kind === 'layers'}
              {@const [sx, sy] = layerScale(sprite)}
              <button
                class="layer stack"
                class:flip={sprite.flip}
                style={`${box};z-index:${i}`}
                aria-label={`Go to show: ${sprite.name}`}
                title={tip}
                onclick={() => openShow(sprite)}
                oncontextmenu={(e) => spriteMenu(e, sprite)}
              >
                {#each sprite.picture.layers as layer, n (`${n}:${layer.path}`)}
                  {@const known = natural[layer.path]}
                  {#if url(layer.path)}
                    <img
                      src={url(layer.path)}
                      alt=""
                      style={known
                        ? `left:${layer.x * sx}px;top:${layer.y * sy}px;width:${known.w * sx}px;height:${known.h * sy}px`
                        : `left:${layer.x * sx}px;top:${layer.y * sy}px;visibility:hidden`}
                      onload={(e) => remember(layer.path, e)}
                      onerror={() => miss(layer.path)}
                    />
                  {/if}
                {/each}
              </button>
            {:else if filePath && url(filePath)}
              <button
                class="layer pic"
                style={`${box};z-index:${i}`}
                aria-label={`Go to show: ${sprite.name}`}
                title={tip}
                onclick={() => openShow(sprite)}
                oncontextmenu={(e) => spriteMenu(e, sprite)}
              >
                <img src={url(filePath)} alt="" class:flip={sprite.flip} onload={(e) => remember(filePath, e)} onerror={() => miss(filePath)} />
              </button>
            {:else}
              <button
                class="layer missing"
                style={`${box};z-index:${i}`}
                aria-label={`Go to show: ${sprite.name}`}
                title={reasonOf(sprite, filePath) || sprite.name}
                onclick={() => openShow(sprite)}
                oncontextmenu={(e) => spriteMenu(e, sprite)}
              >
                {sprite.name}
                {#if reasonOf(sprite, filePath)}<small class="why">{reasonOf(sprite, filePath)}</small>{/if}
              </button>
            {/if}
          {/if}
        {/each}

        {#if estimate.say}
          {@const say = estimate.say}
          <div
            class="window"
            class:plain={!url(gui.textbox)}
            style={`top:${(estimate.height - gui.textboxHeight) * gui.textboxYalign}px;height:${gui.textboxHeight}px`}
          >
            {#if url(gui.textbox)}
              <img
                class="textbox"
                src={url(gui.textbox)}
                alt=""
                style={gui.textbox && natural[gui.textbox] ? `width:${natural[gui.textbox].w}px` : 'width:100%'}
                onload={(e) => remember(gui.textbox, e)}
              />
            {/if}
            {#if say.who}
              <div
                class="namebox"
                style={`left:${gui.nameXpos}px;top:${gui.nameYpos}px;transform:translateX(${-gui.nameXalign * 100}%);font-size:${gui.nameTextSize}px;color:${say.whoColor ?? gui.accentColor};padding:${gui.nameTextSize * 0.1}px ${gui.nameTextSize * 0.2}px${url(gui.namebox) ? `;background-image:url(${url(gui.namebox)})` : ''}`}
              >
                {say.who}
              </div>
            {/if}
            <div
              class="what"
              style={`left:${gui.dialogueXpos}px;top:${gui.dialogueYpos}px;width:${gui.dialogueWidth}px;font-size:${gui.textSize}px;color:${gui.textColor}`}
            >
              {say.what}
            </div>
          </div>
        {/if}

        {#if estimate.choices.length}
          <div class="choices" style={`top:${gui.choiceYpos}px;gap:${gui.choiceSpacing}px`}>
            {#each estimate.choices as choice (choice.line)}
              <button
                class="choice"
                class:cond={choice.conditional}
                style={`width:${gui.choiceWidth}px;min-height:${choiceH}px;font-size:${gui.choiceTextSize}px;color:${gui.choiceTextColor}`}
                title={choice.conditional ? 'Has a condition, so the game may hide it. Opens this choice in the script.' : 'Opens this choice in the script'}
                onclick={() => goTo(choice.file, choice.line)}
              >
                {#if url(gui.choice)}
                  <img class="choice-bg" src={url(gui.choice)} alt="" onload={(e) => remember(gui.choice, e)} />
                {/if}
                <span>{choice.text}</span>
              </button>
            {/each}
          </div>
        {/if}
      </div>
      {#if !estimate.sprites.length && !estimate.say && !estimate.choices.length}
        <p class="empty">Nothing is on screen at this line.</p>
      {/if}
    </div>
    </div>
    {#if notes.length}
      <details>
        <summary>{notes.length} {notes.length === 1 ? 'note' : 'notes'}</summary>
        <ul>
          {#each notes as note, i (i)}
            <li>{note}</li>
          {/each}
        </ul>
      </details>
    {/if}
    {#if estimate.vars.length || extraPins.length}
      <details class="conds" bind:open={condsOpen}>
        <summary>Conditions</summary>
        {#each estimate.vars as variable (variable.name)}
          <div class="cond-row">
            <span class="cond-name">{variable.name}</span>
            <input
              class="cond-value"
              aria-label={`Preview value for ${variable.name}`}
              placeholder={variable.origin === 'script' ? variable.value : 'unset'}
              value={fieldOf(variable.name)}
              title={variable.origin === 'pinned' ? 'Pinned for this project' : variable.origin === 'script' ? 'From the script' : 'Not set in the script'}
              oninput={(e) => (drafts = { ...drafts, [variable.name]: e.currentTarget.value })}
              onkeydown={(e) => {
                if (e.key === 'Enter') commitPin(variable.name, e.currentTarget)
              }}
              onblur={(e) => commitPin(variable.name, e.currentTarget)}
            />
            {#if fieldOf(variable.name)}
              <button class="text" title={`Clear ${variable.name}`} onclick={() => clearPin(variable.name)}>Clear</button>
            {/if}
            {#each variable.uses as use, i (`${use.file}:${use.line}:${i}`)}
              <button
                class="text use"
                title={use.cond || 'Condition'}
                onclick={() => goTo(use.file, use.line)}
              >
                {use.file.split('/').pop()}:{use.line}
              </button>
            {/each}
            {#if variable.more}<span class="dim">+{variable.more} more</span>{/if}
            {#if condErrors[variable.name]}<span class="cond-err">{condErrors[variable.name]}</span>{/if}
          </div>
        {/each}
        {#each extraPins as name (name)}
          <div class="cond-row">
            <span class="cond-name">{name}</span>
            <input
              class="cond-value"
              aria-label={`Preview value for ${name}`}
              placeholder="unset"
              value={fieldOf(name)}
              title="Pinned for this project"
              oninput={(e) => (drafts = { ...drafts, [name]: e.currentTarget.value })}
              onkeydown={(e) => {
                if (e.key === 'Enter') commitPin(name, e.currentTarget)
              }}
              onblur={(e) => commitPin(name, e.currentTarget)}
            />
            <button class="text" title={`Clear ${name}`} onclick={() => clearPin(name)}>Clear</button>
            {#if condErrors[name]}<span class="cond-err">{condErrors[name]}</span>{/if}
          </div>
        {/each}
        {#if estimate.varsMore}<span class="dim">+{estimate.varsMore} more</span>{/if}
      </details>
    {/if}
  {:else}
    <p class="empty">{stageUi.error || (app.cursor || stageUi.pending ? 'Estimating stage…' : 'Move the caret into a script to see the stage.')}</p>
  {/if}
</div>

<style>
  .stage {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    background: var(--panel);
  }
  .head {
    height: 28px;
    padding: 0 8px;
    gap: 8px;
  }
  .mode {
    font-size: var(--fs-sm);
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--dim);
  }
  .mode.live {
    color: var(--ok);
  }
  .dim {
    color: var(--dim);
    font-size: var(--fs-md);
  }
  .status {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .text {
    border: none;
    background: transparent;
    color: var(--dim);
    font-size: var(--fs-md);
    padding: 0 4px;
    border-radius: var(--r-sm);
  }
  .text:hover,
  .text[aria-pressed='true'] {
    color: var(--text);
  }
  .fit {
    flex: 1 1 0;
    min-width: 0;
    min-height: 0;
    display: grid;
    place-items: center;
    container-type: size;
  }
  .frame {
    position: relative;
    width: min(100cqw, calc(100cqh * var(--ar, 1.777778)));
    height: min(100cqh, calc(100cqw / var(--ar, 1.777778)));
    background: var(--stage-frame);
    overflow: hidden;
    border-radius: var(--r-sm);
    box-shadow: 0 0 0 1px var(--line);
  }
  .frame.shot:not([style]) {
    width: auto;
    height: auto;
    max-width: 100cqw;
    max-height: 100cqh;
  }
  .frame.shot img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: contain;
  }
  .frame.shot:not([style]) img {
    width: auto;
    height: auto;
    max-width: 100cqw;
    max-height: 100cqh;
  }
  .world {
    position: absolute;
    left: 0;
    top: 0;
    transform-origin: 0 0;
    font-family: 'DejaVu Sans', 'Segoe UI', sans-serif;
  }
  .layer {
    position: absolute;
    border: none;
    padding: 0;
    background: transparent;
    border-radius: 0;
  }
  .pic img {
    display: block;
    width: 100%;
    height: 100%;
    pointer-events: none;
  }
  .stack img {
    position: absolute;
    pointer-events: none;
  }
  .stack.flip {
    transform: scaleX(-1);
  }
  .pic img.flip {
    transform: scaleX(-1);
  }
  .missing {
    display: grid;
    place-items: center;
    overflow: hidden;
    border: 6px dashed color-mix(in srgb, var(--stage-ink) 40%, transparent);
    color: color-mix(in srgb, var(--stage-ink) 70%, transparent);
    font-size: clamp(10px, 18%, 48px);
    text-align: center;
    background: color-mix(in srgb, var(--stage-ink) 5%, transparent);
  }
  .missing .why {
    display: block;
    font-size: 0.6em;
    opacity: 0.8;
    word-break: break-all;
  }
  .frame.dim {
    opacity: 0.72;
  }
  .tail {
    margin-left: auto;
    display: flex;
    gap: 8px;
    min-width: 0;
  }
  .bad {
    color: var(--error);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 240px;
  }
  .window {
    position: absolute;
    left: 0;
    width: 100%;
    z-index: 10000;
    pointer-events: none;
  }
  .window.plain {
    background: linear-gradient(to bottom, rgba(0, 0, 0, 0.45), rgba(0, 0, 0, 0.8));
  }
  .textbox {
    position: absolute;
    left: 50%;
    bottom: 0;
    transform: translateX(-50%);
  }
  .namebox {
    position: absolute;
    white-space: nowrap;
    background-size: 100% 100%;
    line-height: 1.2;
  }
  .what {
    position: absolute;
    line-height: 1.25;
    white-space: pre-wrap;
    overflow-wrap: break-word;
  }
  .choices {
    position: absolute;
    left: 0;
    width: 100%;
    z-index: 10001;
    display: flex;
    flex-direction: column;
    align-items: center;
    transform: translateY(-50%);
  }
  .choice {
    position: relative;
    display: grid;
    place-items: center;
    border: none;
    border-radius: 0;
    padding: 0 8%;
    background: rgba(0, 0, 0, 0.55);
    line-height: 1.2;
  }
  .choice:has(.choice-bg) {
    background: transparent;
  }
  .choice:hover {
    filter: brightness(1.2);
  }
  .choice.cond span {
    font-style: italic;
  }
  .choice-bg {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
  }
  .choice span {
    position: relative;
  }
  .empty {
    margin: 0;
    padding: 16px;
    color: var(--dim);
    font-size: var(--fs-md);
    text-align: center;
  }
  .frame > .empty {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
  }
  details {
    flex: 0 1 auto;
    max-height: 40%;
    overflow: auto;
    padding: 4px 8px 8px;
    color: var(--dim);
    font-size: var(--fs-md);
  }
  summary {
    cursor: pointer;
  }
  .conds {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .cond-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    min-height: 24px;
  }
  .cond-name {
    font-family: var(--mono, ui-monospace, monospace);
    color: var(--text);
  }
  .cond-value {
    width: 8rem;
    height: 22px;
    padding: 0 6px;
    border: 1px solid var(--line);
    border-radius: var(--r-sm);
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-md);
  }
  .cond-value:focus {
    outline: 1px solid var(--accent, var(--text));
  }
  .use {
    font-family: var(--mono, ui-monospace, monospace);
  }
  .cond-err {
    color: var(--error);
    font-size: var(--fs-sm);
  }
  ul {
    margin: 6px 0 0;
    padding-left: 16px;
  }
  li + li {
    margin-top: 4px;
  }
</style>
