<script lang="ts">
  import { tick, untrack } from 'svelte'
  import PanZoom from './PanZoom.svelte'
  import Hud from './Hud.svelte'
  import ZoomControls from './ZoomControls.svelte'
  import Icon from './Icon.svelte'
  import FlowEditor from './FlowEditor.svelte'
  import { api, errorText } from '../lib/api'
  import { chipCharLimit, chipWidthFor, graphPillText, layoutLabelGraph, pillCharLimit, type GLEdge, type GLNode, type GraphLayout } from '../lib/layout'
  import { flowNodeItems, openContextMenu, type FlowActions } from '../lib/context.svelte'
  import { askText } from '../lib/dialog.svelte'
  import { insertSpot, isBeatCard, isStatementNode, requestKey, type EditorRequest, type InsertKind } from '../lib/flowedit'
  import { canRearrange, deleteNode, duplicateNode, flowPane, moveNode, nodeAtLine } from '../lib/flowops.svelte'
  import { sceneEdit, sceneRedo, sceneUndo } from '../lib/scene.svelte'
  import { setSetting, settings } from '../lib/settings.svelte'
  import { app, fileInfo, nodeByName, symbolsOf } from '../lib/store.svelte'
  import { toggleFlowDetail } from '../lib/nav.svelte'
  import type { BeatLine, GNode, LabelGraph } from '../lib/types'
  import { EDGE_BUDGET, NODE_BUDGET, indexBoxes, intersects, spatialIndex, truncate, type ViewRect } from '../lib/view'

  interface Props {
    name: string
    file: string | null
    cursorLine: number | null
    /** Line the running game is on, when this graph's file is that file. */
    liveLine: number | null
    reloadKey: number
    onopen: (label: string) => void
    ongoto: (file: string, line: number, endLine: number) => void
  }

  let { name, file, cursorLine, liveLine, reloadKey, onopen, ongoto }: Props = $props()

  const EDITOR_W = 320
  const EDITOR_H = 300

  let graph = $state.raw<LabelGraph | null>(null)
  let layout = $state.raw<GraphLayout | null>(null)
  let loading = $state(false)
  let error = $state('')
  let pz: ReturnType<typeof PanZoom> | undefined = $state()
  let view = $state.raw<ViewRect | null>(null)
  let rulerHeight = $state(0)
  let selectedNode = $state<number | null>(null)
  let editor = $state.raw<EditorRequest | null>(null)
  /** Node the editor hangs beside. */
  let editorAt = $state<number | null>(null)
  let token = 0
  let handledFocus = app.flowFocus?.seq ?? 0

  interface Drag {
    from: GNode
    sx: number
    sy: number
    x: number
    y: number
    hover: number | null
  }
  interface Picker {
    x: number
    y: number
    from: GNode
    target: string
    fresh: boolean
    err: string
  }
  let drag = $state<Drag | null>(null)
  let picker = $state<Picker | null>(null)

  const canEdit = $derived.by(() => {
    if (!file) return false
    return fileInfo(file)?.editable !== false
  })
  const writing = $derived(canEdit && app.flowDetail)
  const characters = $derived(symbolsOf('character'))
  const characterNames = $derived(characters.map((c) => c.name))
  const imageNames = $derived(['black', ...symbolsOf('image').map((s) => s.name)])
  const transformNames = $derived(symbolsOf('transform').map((s) => s.name))
  const scenes = $derived(
    (app.map?.nodes ?? []).filter((n) => n.kind === 'label' || n.kind === 'menu').map((n) => n.id),
  )
  const dead = $derived((app.catalog?.deadEnds ?? []).includes(name))
  const unplayedHere = $derived((app.catalog?.endings ?? []).includes(name) && !app.visitedLabels.includes(name))

  $effect(() => {
    const n = name
    const detail = app.flowDetail
    void reloadKey
    const t = ++token
    loading = true
    error = ''
    api
      .labelGraph(n, detail)
      .then(async (g) => {
        const l = await layoutLabelGraph(g)
        if (t !== token) return
        graph = g
        layout = l
        editor = null
        editorAt = null
        picker = null
      })
      .catch((e) => {
        if (t === token) error = errorText(e)
      })
      .finally(() => {
        if (t === token) loading = false
      })
  })

  const nodesById = $derived(new Map<number, GNode>((graph?.nodes ?? []).map((n) => [n.id, n])))
  const posById = $derived(new Map<number, GLNode>((layout?.nodes ?? []).map((n) => [n.id, n])))
  /** Jump-out nodes drawn for a screen's actions. Their line is the screen's, not a jump. */
  const fromScreen = $derived.by(() => {
    const out = new Set<number>()
    const g = graph
    if (!g) return out
    for (const e of g.edges) if (nodesById.get(e.from)?.kind === 'screen') out.add(e.to)
    return out
  })

  const grids = $derived.by(() => {
    const l = layout
    if (!l) return null
    return {
      nodes: spatialIndex(l.nodes.map((n) => ({ x0: n.x, y0: n.y, x1: n.x + n.w, y1: n.y + n.h }))),
      edges: spatialIndex(indexBoxes(l.edges)),
    }
  })

  const missing = $derived.by(() => {
    const g = graph
    const ids = new Set<number>()
    if (!g) return ids
    for (const n of g.nodes) if (isMissing(n)) ids.add(n.id)
    return ids
  })

  const cursorNode = $derived.by(() => {
    if (cursorLine === null || !graph) return null
    return nodeAtLine(graph.nodes, cursorLine)?.id ?? null
  })

  const liveNode = $derived.by(() => {
    if (liveLine === null || !graph) return null
    return nodeAtLine(graph.nodes, liveLine)?.id ?? null
  })

  const selected = $derived(selectedNode === null ? null : (nodesById.get(selectedNode) ?? null))

  let legendOpen = $state(localStorage.getItem('vnide.flowLegend') === '1')
  function toggleLegend() {
    legendOpen = !legendOpen
    localStorage.setItem('vnide.flowLegend', legendOpen ? '1' : '0')
  }
  const LEGEND = [
    ['', 'Dialogue'],
    ['menu', 'Choice'],
    ['cond', 'Condition'],
    ['jump', 'Jump'],
    ['call', 'Call'],
    ['screen', 'Screen'],
    ['missing', 'Missing target'],
    ['live', 'Running now'],
  ] as const
  const followCaret = $derived(settings.followCaret)

  function toggleFollowCaret() {
    setSetting('followCaret', !followCaret)
  }

  /** Screen position for a popover of the given size, hanging off a node. */
  function popStyle(p: { x: number; y: number }, view: ViewRect, width = EDITOR_W, height = EDITOR_H): string {
    const vw = (view.x1 - view.x0) * view.k
    const vh = (view.y1 - view.y0) * view.k
    const left = Math.min(Math.max(8, (p.x - view.x0) * view.k), Math.max(8, vw - width - 8))
    const top = Math.min(Math.max(8, (p.y - view.y0) * view.k), Math.max(8, vh - height - 8))
    return `left:${left}px;top:${top}px`
  }

  function reveal(id: number) {
    const p = posById.get(id)
    if (!p || !pz) return
    const v = pz.currentRect()
    if (!intersects(v, p.x, p.y, p.x + p.w, p.y + p.h, -20)) {
      pz.centerOn(p.x + p.w / 2, p.y + p.h / 2)
    }
  }

  // Follow the code cursor.
  $effect(() => {
    const id = cursorNode
    if (!followCaret || id === null || !pz) return
    untrack(() => reveal(id))
  })

  // Select what a flow edit just touched once the new graph is on screen.
  $effect(() => {
    void app.flowFocus
    void graph
    void loading
    untrack(applyFocus)
  })

  function applyFocus() {
    const f = app.flowFocus
    const g = graph
    if (!f || !g || loading || f.seq === handledFocus || f.file !== file) return
    const n = nodeAtLine(g.nodes, f.line)
    if (!n) return
    handledFocus = f.seq
    selectedNode = n.id
    void tick().then(() => reveal(n.id))
    if (!f.open || !writing) return
    if (isBeatCard(n)) {
      const b = n.beats.find((x) => x.line === f.line)
      if (b) openBeat(n, b)
    } else if (editable(n)) {
      openNodeEditor(n)
    }
  }

  function isMissing(n: GNode): boolean {
    if (n.kind === 'screen') return false
    return !!n.target && !n.dynamic && !nodeByName(n.target)
  }

  function sayCard(n: GNode): boolean {
    return n.kind === 'dialogue' && !!n.body
  }

  /** Cards the writer can open an editor on. */
  function editable(n: GNode): boolean {
    if (fromScreen.has(n.id)) return false
    switch (n.kind) {
      case 'dialogue':
        return !!n.body
      case 'choice':
      case 'menu':
      case 'return':
        return true
      case 'jump':
      case 'call':
        return !n.dynamic
      default:
        return false
    }
  }

  function wrapText(text: string, chars: number): string[] {
    const words = text.replace(/\s+/g, ' ').trim().split(' ').filter(Boolean)
    const lines: string[] = []
    let cur = ''
    for (const word of words) {
      const next = cur ? `${cur} ${word}` : word
      if (cur && next.length > chars) {
        lines.push(cur)
        cur = word
      } else cur = next
    }
    if (cur) lines.push(cur)
    return lines.slice(0, 6)
  }

  function condOf(n: GNode): string {
    return graph?.edges.find((e) => e.to === n.id && e.kind === 'choice')?.cond ?? ''
  }

  function openNodeEditor(n: GNode) {
    selectedNode = n.id
    editorAt = n.id
    editor = { mode: 'node', node: n, cond: condOf(n) }
  }

  function openBeat(n: GNode, b: BeatLine) {
    selectedNode = n.id
    editorAt = n.id
    editor = { mode: 'beat', node: n, beat: b }
  }

  function openInsert(n: GNode, side: 'above' | 'below', kind?: InsertKind) {
    const spot = insertSpot(n, side)
    if (!spot) return
    selectedNode = n.id
    editorAt = n.id
    editor = { mode: 'insert', spot, kind, speaker: n.speakers[0] ?? '' }
  }

  function openTop() {
    const g = graph
    if (!g || g.kind !== 'label') return
    editorAt = g.root
    editor = { mode: 'insert', spot: { line: g.line, place: 'into', places: ['into'] } }
  }

  function plusClick(n: GNode, e: Event) {
    e.stopPropagation()
    if (!writing) return
    if (n.kind === 'menu') openInsert(n, 'below', 'choice')
    else openInsert(n, 'below')
  }

  function clickNode(n: GNode) {
    selectedNode = n.id
    if (writing && editable(n)) {
      if (file) ongoto(file, n.line, n.endLine)
      openNodeEditor(n)
      return
    }
    editor = null
    if (n.kind === 'screen' && n.target && nodeByName('screen:' + n.target)) {
      onopen('screen:' + n.target)
      return
    }
    if ((n.kind === 'jump' || n.kind === 'fall' || n.kind === 'call') && n.target && !n.dynamic) {
      if (nodeByName(n.target)) {
        onopen(n.target)
        return
      }
    }
    if (file) ongoto(file, n.line, n.endLine)
  }

  function clickBeat(n: GNode, b: BeatLine, e: Event) {
    e.stopPropagation()
    selectedNode = n.id
    if (file) ongoto(file, b.line, b.endLine)
    if (writing) openBeat(n, b)
  }

  function unknownSpeaker(n: GNode): boolean {
    const who = n.speakers[0]
    return sayCard(n) && !!who && !characters.some((c) => c.name === who)
  }

  async function createScene(sceneName: string): Promise<boolean> {
    if (!file || !canEdit) return false
    return sceneEdit({ op: 'add-label', path: file, name: sceneName })
  }

  async function newScene() {
    if (!file || !canEdit) return
    const sceneName = await askText('Name of the new scene', '', 'Create')
    if (!sceneName) return
    if (await sceneEdit({ op: 'add-label', path: file, name: sceneName })) onopen(sceneName)
  }

  async function addReturn() {
    if (!file || !graph) return
    await sceneEdit({ op: 'link-scene', path: file, line: graph.line, how: 'return' })
  }

  async function addJumpOut() {
    if (!file || !graph) return
    const target = await askText('Jump to which scene?', '', 'Add jump')
    if (!target) return
    await sceneEdit({
      op: 'link-scene',
      path: file,
      line: graph.line,
      how: 'jump',
      target,
      newLabel: !scenes.includes(target),
    })
  }

  function actionsFor(n: GNode): FlowActions | undefined {
    if (!file || !writing || !isStatementNode(n) || fromScreen.has(n.id)) return undefined
    const path = file
    const rearrange = canRearrange(n)
    return {
      edit: editable(n) ? () => openNodeEditor(n) : undefined,
      insert: (side) => openInsert(n, side),
      canInsert: (side) => !!insertSpot(n, side),
      duplicate: rearrange ? () => void duplicateNode(path, n) : undefined,
      move: rearrange ? (dir) => void moveNode(path, n, dir) : undefined,
      remove: rearrange ? () => void deleteNode(path, n) : undefined,
    }
  }

  function nodeMenu(ev: MouseEvent, n: GNode) {
    selectedNode = n.id
    const items = flowNodeItems(file, n, actionsFor(n))
    if (writing && isMissing(n) && n.target) {
      const target = n.target
      items.push({ kind: 'item', label: `Create scene ${target}`, run: () => void createScene(target) })
    }
    openContextMenu(ev, items)
  }

  // Dragging a handle onto another scene rewires the jump or choice it came from.

  function handleShown(n: GNode): boolean {
    if (!writing || fromScreen.has(n.id)) return false
    if (n.kind === 'choice') return n.targetLine > 0
    return (n.kind === 'jump' || n.kind === 'call') && !n.dynamic
  }

  function dropScene(n: GNode): string | null {
    if (n.kind !== 'jump' && n.kind !== 'fall' && n.kind !== 'call' && n.kind !== 'choice') return null
    if (fromScreen.has(n.id)) return null
    return n.target && !n.dynamic ? n.target : null
  }

  function dropTarget(x: number, y: number, from: GNode): GNode | null {
    for (const p of layout?.nodes ?? []) {
      if (p.id === from.id || x < p.x || x > p.x + p.w || y < p.y || y > p.y + p.h) continue
      const n = nodesById.get(p.id)
      if (n && dropScene(n)) return n
    }
    return null
  }

  function startDrag(n: GNode, e: PointerEvent) {
    e.stopPropagation()
    e.preventDefault()
    const p = posById.get(n.id)
    if (!p || !pz || e.button !== 0) return
    selectedNode = n.id
    editor = null
    picker = null
    const sx = p.x + p.w / 2
    const sy = p.y + p.h
    drag = { from: n, sx, sy, x: sx, y: sy, hover: null }
    const move = (ev: PointerEvent) => {
      if (!drag || !pz) return
      const pt = pz.toContent(ev.clientX, ev.clientY)
      drag = { ...drag, x: pt.x, y: pt.y, hover: dropTarget(pt.x, pt.y, drag.from)?.id ?? null }
    }
    const cleanup = () => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', stop)
      window.removeEventListener('keydown', cancel, true)
    }
    const cancel = (ev: KeyboardEvent) => {
      if (ev.key !== 'Escape') return
      ev.stopPropagation()
      cleanup()
      drag = null
    }
    const stop = (ev: PointerEvent) => {
      cleanup()
      const d = drag
      drag = null
      if (!d || !pz) return
      const pt = pz.toContent(ev.clientX, ev.clientY)
      const hit = dropTarget(pt.x, pt.y, d.from)
      if (hit?.target) {
        void rewire(d.from, hit.target, false)
        return
      }
      const k = pz.currentRect().k
      if (Math.hypot(pt.x - d.sx, pt.y - d.sy) * k < 12) return
      picker = { x: pt.x, y: pt.y, from: d.from, target: '', fresh: false, err: '' }
    }
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', stop)
    window.addEventListener('keydown', cancel, true)
  }

  async function rewire(from: GNode, target: string, fresh: boolean): Promise<boolean> {
    if (!file) return false
    // A choice keeps its caption: only the jump line under it changes.
    const line = from.kind === 'choice' ? from.targetLine : from.line
    return sceneEdit({ op: 'set-jump', path: file, line, target, newLabel: fresh })
  }

  async function applyPicker() {
    const p = picker
    if (!p) return
    const target = p.target.trim()
    if (!target) {
      p.err = 'Name the scene to connect to.'
      picker = { ...p }
      return
    }
    const fresh = p.fresh || !scenes.includes(target)
    picker = null
    await rewire(p.from, target, fresh)
  }

  function flowKey(e: KeyboardEvent) {
    const t = e.target
    if (t instanceof HTMLElement && t.closest('input, textarea, select, form')) return
    const mod = e.ctrlKey || e.metaKey
    if (mod && !e.altKey && e.key.toLowerCase() === 'z') {
      e.preventDefault()
      void (e.shiftKey ? sceneRedo() : sceneUndo())
      return
    }
    if (mod && !e.altKey && !e.shiftKey && e.key.toLowerCase() === 'y') {
      e.preventDefault()
      void sceneRedo()
      return
    }
    if (e.key === 'Escape' && (editor || picker)) {
      e.preventDefault()
      e.stopPropagation()
      editor = null
      picker = null
      return
    }
    const n = selected
    if (!writing || !file || !n || !canRearrange(n) || fromScreen.has(n.id)) return
    if (e.key === 'Delete' && !mod && !e.altKey) {
      e.preventDefault()
      void deleteNode(file, n)
    } else if (e.altKey && !mod && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
      e.preventDefault()
      void moveNode(file, n, e.key === 'ArrowUp' ? -1 : 1)
    }
  }

  function nodeLabel(n: GNode): string {
    return graphPillText(n)
  }

  function rangeText(n: GNode): string {
    return n.endLine > n.line ? `lines ${n.line}–${n.endLine}` : `line ${n.line}`
  }

  interface Marker {
    id: number
    kind: string
    top: number
  }

  const markers = $derived.by<Marker[]>(() => {
    if (!layout || !graph || rulerHeight <= 0) return []
    const out: Marker[] = []
    for (const n of graph.nodes) {
      const p = posById.get(n.id)
      if (!p) continue
      let kind = ''
      if (n.id === graph.root) kind = 'start'
      else if (n.kind === 'menu') kind = 'menu'
      else if ((n.kind === 'jump' || n.kind === 'call' || n.kind === 'fall') && missing.has(n.id)) kind = 'missing'
      else if (n.kind === 'jump' || n.kind === 'fall') kind = 'out'
      else if (n.kind === 'return' || n.kind === 'end') kind = 'end'
      if (!kind) continue
      out.push({ id: n.id, kind, top: ((p.y + p.h / 2) / layout.height) * rulerHeight })
    }
    return out
  })

  const band = $derived.by(() => {
    if (!layout || !view || rulerHeight <= 0) return { top: 0, height: 0 }
    const span = Math.max(1, layout.height)
    const top = Math.max(0, (view.y0 / span) * rulerHeight)
    const bottom = Math.min(rulerHeight, (view.y1 / span) * rulerHeight)
    return { top, height: Math.max(6, bottom - top) }
  })

  function rulerClick(e: MouseEvent) {
    if (!layout || !pz) return
    const box = (e.currentTarget as HTMLElement).getBoundingClientRect()
    const y = ((e.clientY - box.top) / box.height) * layout.height
    pz.centerOn(layout.width / 2, y)
  }

  function packEdges(ids: number[], selected: number | null): { batches: { kind: string; d: string }[]; hot: string } {
    const g = graph
    const l = layout
    const batches: { kind: string; d: string }[] = []
    if (!g || !l) return { batches, hot: '' }
    const parts = new Map<string, string[]>()
    let hot = ''
    for (const i of ids) {
      const e = l.edges[i]
      const ge = g.edges[e.index]
      if (!ge) continue
      if (selected !== null && (ge.from === selected || ge.to === selected)) {
        hot = hot ? `${hot} ${e.d}` : e.d
        continue
      }
      const bucket = parts.get(ge.kind)
      if (bucket) bucket.push(e.d)
      else parts.set(ge.kind, [e.d])
    }
    for (const [kind, ds] of parts) batches.push({ kind, d: ds.join(' ') })
    return { batches, hot }
  }

  function edgeClass(e: GLEdge): string {
    return graph?.edges[e.index]?.kind ?? 'next'
  }

  const edgeSummary = $derived(
    graph
      ? `${graph.nodes.length} nodes · ${graph.nodes.reduce((a, n) => a + n.says, 0)} lines`
      : '',
  )
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="lg"
  onkeydown={flowKey}
  onpointerdowncapture={() => (flowPane.active = true)}
  onfocusin={() => (flowPane.active = true)}
>
  {#if layout && graph}
    {@const L = layout}
    {@const G = graph}
    <PanZoom
      bind:this={pz}
      contentWidth={L.width}
      contentHeight={L.height}
      fitKey={`${G.name}|${app.flowDetail ? 'd' : 'o'}`}
      minK={0.05}
      maxK={2.5}
      onview={(r) => (view = r)}
    >
      {#snippet children(v)}
        {@const nodeIds = grids ? grids.nodes.query(v.x0, v.y0, v.x1, v.y1) : []}
        {@const edgeIds = grids ? grids.edges.query(v.x0 - 20, v.y0 - 20, v.x1 + 20, v.y1 + 20) : []}
        {@const cheap = nodeIds.length > NODE_BUDGET || edgeIds.length > EDGE_BUDGET}
        {@const batch = edgeIds.length > EDGE_BUDGET}
        {@const packed = batch ? packEdges(edgeIds, selectedNode) : null}
        {@const showPreview = !cheap && v.k >= 0.35}
        {@const showChips = !cheap && v.k >= 0.4}
        <defs>
          {#each ['next', 'choice', 'branch'] as kind (kind)}
            <marker
              id={`g-arrow-${kind}`}
              viewBox="0 0 10 10"
              refX="9"
              refY="5"
              markerWidth="9"
              markerHeight="9"
              markerUnits="userSpaceOnUse"
              orient="auto"
            >
              <path d="M0 0 L10 5 L0 10 z" class={`arrow ${kind}`} />
            </marker>
          {/each}
        </defs>

        {#if packed}
          {#each packed.batches as b (b.kind)}
            <path d={b.d} class={`edge ${b.kind}`} />
          {/each}
          {#if packed.hot}
            <path d={packed.hot} class="edge hot" />
          {/if}
        {:else}
          {#each edgeIds as i (L.edges[i].index)}
            {@const e = L.edges[i]}
            <path
              d={e.d}
              class={`edge ${edgeClass(e)}`}
              marker-end={cheap ? undefined : `url(#g-arrow-${edgeClass(e)})`}
            />
          {/each}
        {/if}

        {#each nodeIds as i (L.nodes[i].id)}
          {@const p = L.nodes[i]}
          {@const n = nodesById.get(p.id)}
          {#if n}
            <g
              class={`node ${n.kind}`}
              class:cursor={cursorNode === n.id}
              class:live={liveNode === n.id}
              class:selected={selectedNode === n.id}
              class:missing={missing.has(n.id)}
              class:dynamic={n.dynamic}
              class:drop={drag !== null && drag.hover === n.id}
              role="button"
              tabindex="-1"
              onclick={() => clickNode(n)}
              onkeydown={(ev) => ev.key === 'Enter' && clickNode(n)}
              oncontextmenu={(ev) => nodeMenu(ev, n)}
            >
              <rect x={p.x} y={p.y} width={p.w} height={p.h} rx={sayCard(n) || n.kind === 'choice' || isBeatCard(n) ? 8 : p.h / 2} />
              {#if !cheap && sayCard(n)}
                <text x={p.x + 12} y={p.y + 18} class="head">{n.speakers[0] || 'narrator'}</text>
                {#each wrapText(n.body, 38) as line, li (li)}
                  <text x={p.x + 12} y={p.y + 36 + li * 16} class="preview">{line}</text>
                {/each}
                {#if unknownSpeaker(n)}<text x={p.x + 12} y={p.y + p.h - 8} class="badges">no character</text>{/if}
                {#if writing}
                  <g class="plus" role="button" tabindex="0" onclick={(e) => plusClick(n, e)} onkeydown={(e) => e.key === 'Enter' && plusClick(n, e)}>
                    <title>Add after this line</title>
                    <circle cx={p.x + p.w - 16} cy={p.y + 16} r="8" />
                    <text x={p.x + p.w - 16} y={p.y + 20} text-anchor="middle">+</text>
                  </g>
                {/if}
              {:else if !cheap && n.kind === 'choice'}
                {#each wrapText(n.body || n.title, 38) as line, li (li)}
                  <text x={p.x + 12} y={p.y + 18 + li * 16} class="preview">{line}</text>
                {/each}
                {#if n.target}<text x={p.x + 12} y={p.y + p.h - 10} class="head">{n.target}</text>{/if}
                {#if missing.has(n.id)}<text x={p.x + p.w - 12} y={p.y + p.h - 10} text-anchor="end" class="badges">missing</text>{/if}
                {#if writing && !fromScreen.has(n.id)}
                  <g class="plus" role="button" tabindex="0" onclick={(e) => plusClick(n, e)} onkeydown={(e) => e.key === 'Enter' && plusClick(n, e)}>
                    <title>Add inside this choice</title>
                    <circle cx={p.x + p.w - 16} cy={p.y + 16} r="8" />
                    <text x={p.x + p.w - 16} y={p.y + 20} text-anchor="middle">+</text>
                  </g>
                {/if}
              {:else if !cheap && isBeatCard(n)}
                {#each n.beats as b, bi (b.line)}
                  <g
                    class="beat"
                    class:off={!b.editable}
                    class:active={editor?.mode === 'beat' && editor.beat.line === b.line}
                    role="button"
                    tabindex="-1"
                    onclick={(e) => clickBeat(n, b, e)}
                    onkeydown={(e) => e.key === 'Enter' && clickBeat(n, b, e)}
                  >
                    <rect x={p.x + 4} y={p.y + 8 + bi * 18} width={p.w - 8} height="18" rx="4" class="beat-hit" />
                    <text x={p.x + 12} y={p.y + 21 + bi * 18} class="preview">
                      <tspan class="cmd">{b.cmd}</tspan>
                      {truncate(b.text, 32)}
                    </text>
                  </g>
                {/each}
                {#if writing}
                  <g class="plus" role="button" tabindex="0" onclick={(e) => plusClick(n, e)} onkeydown={(e) => e.key === 'Enter' && plusClick(n, e)}>
                    <title>Add after this</title>
                    <circle cx={p.x + p.w - 16} cy={p.y + 16} r="8" />
                    <text x={p.x + p.w - 16} y={p.y + 20} text-anchor="middle">+</text>
                  </g>
                {/if}
              {:else if !cheap && n.kind === 'dialogue'}
                <text x={p.x + 12} y={p.y + 18} class="head">
                  {n.says === 0 && n.title ? truncate(n.title, 36) : `${rangeText(n)} · ${n.says} line${n.says === 1 ? '' : 's'}`}
                </text>
                {#if showPreview}
                  {#each n.preview as line, i (i)}
                    <text x={p.x + 12} y={p.y + 36 + i * 15} class="preview">{truncate(line, 44)}</text>
                  {/each}
                {/if}
                {#if n.variants || n.conds || n.python}
                  <text x={p.x + 12} y={p.y + p.h - 8} class="badges">
                    {#if n.variants}⎇ {n.variants} variant{n.variants === 1 ? '' : 's'}  {/if}{#if n.conds}if×{n.conds}  {/if}{#if n.python}py×{n.python}{/if}
                  </text>
                {/if}
              {:else if !cheap}
                {@const full = nodeLabel(n)}
                {@const shown = truncate(full, pillCharLimit(p.w))}
                <text x={p.x + p.w / 2} y={p.y + p.h / 2 + 4.5} text-anchor="middle" class="pill">
                  {shown}
                  {#if shown !== full}<title>{full}</title>{/if}
                </text>
                {#if writing && n.kind === 'menu'}
                  <g class="plus" role="button" tabindex="0" onclick={(e) => plusClick(n, e)} onkeydown={(e) => e.key === 'Enter' && plusClick(n, e)}>
                    <title>Add a choice</title>
                    <circle cx={p.x + p.w - 16} cy={p.y + p.h / 2} r="8" />
                    <text x={p.x + p.w - 16} y={p.y + p.h / 2 + 4} text-anchor="middle">+</text>
                  </g>
                {/if}
              {/if}
              {#if !cheap && handleShown(n)}
                <!-- svelte-ignore a11y_click_events_have_key_events -->
                <g
                  class="handle"
                  role="button"
                  tabindex="-1"
                  onpointerdown={(e) => startDrag(n, e)}
                  onclick={(e) => e.stopPropagation()}
                >
                  <title>Drag onto another scene to connect</title>
                  <circle cx={p.x + p.w / 2} cy={p.y + p.h} r="6" />
                </g>
              {/if}
            </g>
          {/if}
        {/each}

        {#if showChips}
          {#each edgeIds as i (L.edges[i].index)}
            {@const e = L.edges[i]}
            {#if e.label}
              {@const w = e.lw || chipWidthFor(e.label)}
              {@const text = truncate(e.label, chipCharLimit(w))}
              <g class={`chip ${edgeClass(e)}`} pointer-events="none">
                <rect x={e.lx - w / 2} y={e.ly - 9} width={w} height="18" rx="9" />
                <text x={e.lx} y={e.ly + 4} text-anchor="middle">
                  {text}
                  {#if text !== e.label}<title>{e.label}</title>{/if}
                </text>
              </g>
            {/if}
          {/each}
        {/if}
      {/snippet}
      {#snippet hud(v)}
        <Hud>
          <strong class="gname">{G.name}</strong>
          <span class="info">{G.kind} · {edgeSummary}</span>
          <button class:on={followCaret} onclick={toggleFollowCaret} title="Keep the node under the caret in view">Follow caret</button>
          <button class:on={app.flowDetail} onclick={toggleFlowDetail} title="Detail shows every line and lets you write here">{app.flowDetail ? 'Detail' : 'Overview'}</button>
          <ZoomControls k={v.k} onfit={() => pz?.fit()} onin={() => pz?.zoomBy(1.4)} onout={() => pz?.zoomBy(1 / 1.4)} />
          <button class:on={legendOpen} aria-pressed={legendOpen} onclick={toggleLegend} title="Show what the node colors mean">Legend</button>
          {#if canEdit}
            <button disabled={!app.sceneCanUndo || !!app.busy} onclick={() => void sceneUndo()} title="Undo the last edit made from the flow (Ctrl+Z)">Undo</button>
            <button disabled={!app.sceneCanRedo || !!app.busy} onclick={() => void sceneRedo()} title="Redo (Ctrl+Shift+Z)">Redo</button>
            <button onclick={() => void newScene()} title="Add a new scene to this file">New scene</button>
            {#if app.flowDetail && G.kind === 'label'}
              <button onclick={openTop} title="Add a line at the start of this scene">Add at top</button>
            {/if}
          {/if}
          {#if G.nodes.some((n) => n.variants)}
            <span class="info" title="Cosmetic if/elif chains (show/hide/scene/with only) are folded into the dialogue node">⎇ = folded variants</span>
          {/if}
          {#if dead}
            <span class="info">Ends without a jump or a return.</span>
            {#if canEdit && G.kind === 'label'}
              <button onclick={() => void addReturn()}>Add return</button>
              <button onclick={() => void addJumpOut()}>Add jump…</button>
            {/if}
          {/if}
          {#if unplayedHere}<span class="info">Not played yet.</span>{/if}
        </Hud>
        {#if legendOpen}
          <div class="legend anim-pop" role="note" aria-label="Legend">
            {#each LEGEND as [kind, name] (name)}
              <span class="lg-item"><i class={`sw ${kind}`}></i>{name}</span>
            {/each}
          </div>
        {/if}
        {#if drag}
          {@const x1 = (drag.sx - v.x0) * v.k}
          {@const y1 = (drag.sy - v.y0) * v.k}
          {@const x2 = (drag.x - v.x0) * v.k}
          {@const y2 = (drag.y - v.y0) * v.k}
          <svg class="drag-layer" aria-hidden="true">
            <path d={`M${x1} ${y1} C${x1} ${y1 + 60} ${x2} ${y2 - 60} ${x2} ${y2}`} class:ok={drag.hover !== null} />
            <circle cx={x2} cy={y2} r="4" class:ok={drag.hover !== null} />
          </svg>
        {/if}
        {#if editor && file && editorAt !== null && view}
          {@const p = posById.get(editorAt)}
          {#if p}
            {#key requestKey(editor)}
              <FlowEditor
                {file}
                request={editor}
                {canEdit}
                characters={characterNames}
                {scenes}
                images={imageNames}
                transforms={transformNames}
                missingTarget={editor.mode === 'node' && missing.has(editor.node.id) ? (editor.node.target ?? '') : ''}
                style={popStyle(p, view)}
                onclose={() => (editor = null)}
                onrequest={(r) => (editor = r)}
                onopen={onopen}
                oncreate={createScene}
                {ongoto}
              />
            {/key}
          {/if}
        {/if}
        {#if picker && view}
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <form
            class="picker"
            style={popStyle(picker, view, 280, 150)}
            onpointerdown={(e) => e.stopPropagation()}
            onwheel={(e) => e.stopPropagation()}
            onkeydown={(e) => {
              if (e.key === 'Escape') {
                e.stopPropagation()
                picker = null
              }
            }}
            onsubmit={(e) => {
              e.preventDefault()
              void applyPicker()
            }}
          >
            <strong>Connect to a scene</strong>
            <!-- svelte-ignore a11y_autofocus -->
            <input bind:value={picker.target} list="flow-pick-scenes" placeholder="Scene name" aria-label="Scene to connect to" autofocus />
            <datalist id="flow-pick-scenes">
              {#each scenes as scene (scene)}
                <option value={scene}></option>
              {/each}
            </datalist>
            <label><input type="checkbox" bind:checked={picker.fresh} /> New scene</label>
            {#if picker.err}<p class="err">{picker.err}</p>{/if}
            <div class="row">
              <button type="submit" class="primary">Connect</button>
              <button type="button" onclick={() => (picker = null)}>Cancel</button>
            </div>
          </form>
        {/if}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="ruler" bind:clientHeight={rulerHeight} onclick={rulerClick}>
          <div class="band" style={`top:${band.top}px;height:${band.height}px`}></div>
          {#each markers as m (m.id)}
            <div class={`mark ${m.kind}`} style={`top:${m.top}px`}></div>
          {/each}
        </div>
      {/snippet}
    </PanZoom>
  {/if}
  {#if loading && !layout}
    <div class="spinner-overlay" role="status"><div class="spinner"></div>Building graph…</div>
  {/if}
  {#if error}
    <div class="spinner-overlay err" role="alert"><Icon name="error" size={18} />{error}</div>
  {/if}
</div>

<style>
  .lg {
    position: relative;
    width: 100%;
    height: 100%;
  }
  .spinner-overlay.err {
    color: var(--error);
  }
  .gname {
    font-size: var(--fs-md);
    padding: 0 var(--sp-2);
  }
  .legend {
    position: absolute;
    left: 10px;
    bottom: 10px;
    z-index: 2;
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-2) var(--sp-4);
    max-width: calc(100% - 44px);
    padding: var(--sp-3) var(--sp-4);
    background: color-mix(in srgb, var(--panel) 92%, transparent);
    border: 1px solid var(--line);
    border-radius: var(--r-lg);
    box-shadow: var(--shadow-sm);
    font-size: var(--fs-sm);
    color: var(--dim);
  }
  .lg-item {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .sw {
    width: 12px;
    height: 12px;
    border-radius: var(--r-sm);
    border: 2px solid var(--node-stroke);
    background: var(--node-fill);
    box-sizing: border-box;
  }
  .sw.menu { border-color: var(--menu); }
  .sw.cond { border-color: var(--warning); }
  .sw.jump { border-color: var(--accent); }
  .sw.call { border-color: var(--call); }
  .sw.screen { border-color: var(--screen); }
  .sw.missing { border-color: var(--error); }
  .sw.live { border-color: var(--ok); border-style: dashed; }
  .picker {
    position: absolute;
    z-index: 5;
    width: 280px;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px;
    background: var(--panel);
    border: 1px solid var(--accent);
    border-radius: var(--r-lg);
    box-shadow: var(--shadow-lg);
  }
  .picker strong {
    font-size: var(--fs-md);
  }
  .picker input:not([type='checkbox']) {
    width: 100%;
    box-sizing: border-box;
  }
  .picker p,
  .picker label {
    margin: 0;
    color: var(--dim);
    font-size: var(--fs-md);
  }
  .picker .err {
    color: var(--error);
  }
  .picker .row {
    display: flex;
    gap: 6px;
  }
  .drag-layer {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
    z-index: 3;
  }
  .drag-layer path {
    fill: none;
    stroke: var(--accent);
    stroke-width: 2;
    stroke-dasharray: 6 4;
  }
  .drag-layer circle {
    fill: var(--accent);
  }
  .drag-layer .ok {
    stroke: var(--ok);
  }
  .drag-layer circle.ok {
    fill: var(--ok);
  }
  .plus {
    cursor: pointer;
  }
  .plus circle {
    fill: var(--panel);
    stroke: var(--accent);
  }
  .plus text {
    fill: var(--text);
    font-size: 14px;
    pointer-events: none;
  }
  .handle {
    cursor: crosshair;
  }
  .handle circle {
    fill: var(--panel);
    stroke: var(--accent);
    stroke-width: 1.6;
    opacity: 0;
  }
  .node:hover .handle circle,
  .node.selected .handle circle {
    opacity: 1;
  }
  .handle:hover circle {
    fill: var(--accent);
  }
  .beat {
    cursor: pointer;
  }
  .node .beat .beat-hit {
    fill: transparent;
    stroke: none;
  }
  .node .beat:hover .beat-hit,
  .node .beat.active .beat-hit {
    fill: color-mix(in srgb, var(--accent) 18%, transparent);
  }
  .beat.off text {
    opacity: 0.7;
  }
  .cmd {
    fill: var(--accent);
    font-weight: 600;
  }
  .node.choice rect {
    stroke: var(--choice, var(--menu));
    fill: color-mix(in srgb, var(--menu) 14%, var(--node-fill));
  }
  .ruler {
    position: absolute;
    top: 10px;
    bottom: 10px;
    right: 8px;
    width: 14px;
    background: color-mix(in srgb, var(--panel) 85%, transparent);
    border: 1px solid var(--line);
    border-radius: var(--r-md);
    cursor: pointer;
  }
  .band {
    position: absolute;
    left: 0;
    right: 0;
    background: color-mix(in srgb, var(--accent) 22%, transparent);
    border-radius: var(--r-md);
  }
  .mark {
    position: absolute;
    left: 2px;
    right: 2px;
    height: 4px;
    margin-top: -2px;
    border-radius: 2px;
    background: var(--dim);
  }
  .mark.start {
    background: var(--ok);
  }
  .mark.menu {
    background: var(--menu);
  }
  .mark.out {
    background: var(--accent);
  }
  .mark.missing {
    background: var(--error);
    height: 6px;
  }
  .mark.end {
    background: var(--dim);
  }

  .edge {
    fill: none;
    stroke: var(--edge);
    stroke-width: 1.4;
  }
  .edge.choice {
    stroke: var(--choice);
  }
  .edge.branch {
    stroke: var(--warning);
    stroke-dasharray: 5 3;
  }
  .edge.hot {
    stroke: var(--accent);
    stroke-width: 2.4;
    stroke-dasharray: none;
  }
  .arrow {
    fill: var(--edge);
  }
  .arrow.choice {
    fill: var(--choice);
  }
  .arrow.branch {
    fill: var(--warning);
  }

  .chip rect {
    fill: var(--panel);
    stroke: var(--line);
  }
  .chip.choice rect {
    stroke: var(--choice);
  }
  .chip.branch rect {
    stroke: var(--warning);
  }
  .chip text {
    fill: var(--text);
    font-size: 11px;
  }

  .node {
    cursor: pointer;
  }
  .node rect {
    fill: var(--node-fill);
    stroke: var(--node-stroke);
    stroke-width: 1.4;
  }
  .node:hover rect {
    stroke: var(--accent);
  }
  .node.menu rect {
    stroke: var(--menu);
    fill: color-mix(in srgb, var(--menu) 14%, var(--node-fill));
  }
  .node.cond rect {
    stroke: var(--warning);
    fill: color-mix(in srgb, var(--warning) 10%, var(--node-fill));
  }
  .node.jump rect,
  .node.fall rect,
  .node.call rect {
    stroke: var(--accent);
    fill: color-mix(in srgb, var(--accent) 12%, var(--node-fill));
  }
  .node.call rect {
    stroke: var(--call);
  }
  .node.screen rect {
    stroke: var(--screen);
    fill: color-mix(in srgb, var(--screen) 12%, var(--node-fill));
  }
  .node.missing rect {
    stroke: var(--error);
    fill: color-mix(in srgb, var(--error) 18%, var(--node-fill));
  }
  .node.dynamic rect {
    stroke-dasharray: 4 3;
  }
  .node.return rect,
  .node.end rect {
    stroke: var(--dim);
  }
  .node.cursor rect {
    stroke: var(--ok);
    stroke-width: 3;
  }
  .node.live rect {
    stroke: var(--ok);
    stroke-width: 4;
    stroke-dasharray: 5 3;
  }
  .node.selected rect {
    stroke: var(--accent);
    stroke-width: 2.6;
  }
  .node.drop rect {
    stroke: var(--ok);
    stroke-width: 3;
  }
  .head {
    fill: var(--text);
    font-size: 12px;
    font-weight: 600;
    pointer-events: none;
  }
  .preview {
    fill: var(--dim);
    font-size: 11.5px;
    pointer-events: none;
  }
  .badges {
    fill: var(--warning);
    font-size: 10.5px;
    pointer-events: none;
  }
  .pill {
    fill: var(--text);
    font-size: 12px;
    pointer-events: none;
  }
</style>
