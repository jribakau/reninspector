<script lang="ts">
  import { untrack } from 'svelte'
  import PanZoom from './PanZoom.svelte'
  import Hud from './Hud.svelte'
  import Icon from './Icon.svelte'
  import ZoomControls from './ZoomControls.svelte'
  import {
    clusterHue,
    layoutProjectMap,
    mapCacheKey,
    type ClusterMode,
    type MapLayout,
  } from '../lib/layout'
  import { DETAIL_EDGES, NODE_BUDGET, NODE_RECT_BUDGET, batchPaths, indexBoxes, intersects, queryItems, spatialIndex, truncate, type SpatialIndex, type ViewRect } from '../lib/view'
  import { labelItems, openContextMenu } from '../lib/context.svelte'
  import { askText } from '../lib/dialog.svelte'
  import { fileInfo, fileOfNode, nodeByName } from '../lib/indexes.svelte'
  import { sceneEdit } from '../lib/scene.svelte'
  import { app } from '../lib/store.svelte'
  import type { FileInfo, MapNode, ProjectMap } from '../lib/types'

  interface Props {
    map: ProjectMap
    files: FileInfo[]
    root: string
    selected: string | null
    onselect: (name: string) => void
    onopen: (name: string) => void
  }

  let { map, files, root, selected, onselect, onopen }: Props = $props()

  let mode = $state<ClusterMode>('prefix')
  let layout = $state.raw<MapLayout | null>(null)
  let loading = $state(false)
  let progress = $state('')
  let error = $state('')
  let attempt = $state(0)
  /** Attempt number of the layout currently running or last started. Set when a run begins, so an in-flight layout is not restarted. */
  let startedAttempt = -1
  let layoutGen = $state(0)
  let pz: ReturnType<typeof PanZoom> | undefined = $state()
  let token = 0
  let shownKey = ''
  let pendingKey = ''

  const fitKey = $derived(`${root}|${mode}|${map.nodes.length}|${map.edges.length}`)

  $effect(() => {
    const m = map
    const md = mode
    const r = root
    const f = files
    const key = mapCacheKey(r, m, md)
    const tryNo = attempt
    // A layout already on screen stays up when the graph is unchanged. A layout
    // still running for this key is left alone, but switching back to the key on
    // screen must cancel that run or its result would replace the current one.
    // `startedAttempt` is the run that began, so a retry still in flight is not
    // launched again. Comparing with the finished attempt missed that window.
    if (tryNo === startedAttempt && key === pendingKey) return
    if (tryNo === startedAttempt && key === shownKey && pendingKey === '') return
    startedAttempt = tryNo
    pendingKey = key
    const t = ++token
    loading = true
    error = ''
    progress = 'Laying out…'
    void layoutProjectMap(m, f, md, key, (done, total) => {
      if (t === token) progress = `Laying out clusters ${done}/${total}`
    })
      .then((l) => {
        if (t !== token) return
        layout = l
        shownKey = key
        layoutGen += 1
      })
      .catch((e) => {
        if (t === token) error = String(e)
      })
      .finally(() => {
        if (t !== token) return
        if (pendingKey === key) pendingKey = ''
        loading = false
      })
  })

  interface DrawNode {
    id: string
    x: number
    y: number
    w: number
    h: number
    c: number
    cls: string
    tint: string | undefined
    title: string
    sub: string
  }

  interface DrawCluster {
    i: number
    key: string
    x: number
    y: number
    w: number
    h: number
    count: number
    fill: string
    stroke: string
    labelFill: string
  }

  interface DrawEdge {
    i: number
    kind: string
    d: string
    sw: number
    intra: boolean
    from: string
    to: string
    fromC: number
    toC: number
  }

  interface DrawClusterEdge {
    i: number
    from: number
    to: number
    d: string
    sw: number
  }

  interface Prepared {
    nodes: DrawNode[]
    clusters: DrawCluster[]
    edges: DrawEdge[]
    clusterEdges: DrawClusterEdge[]
    nodeGrid: SpatialIndex
    clusterGrid: SpatialIndex
    edgeGrid: SpatialIndex
    clusterEdgeGrid: SpatialIndex
  }

  function nodeClass(n: MapNode | undefined): string {
    if (!n) return ''
    const c: string[] = [n.kind]
    if (!n.reachable && n.kind !== 'missing') c.push('unreachable')
    if (n.duplicate) c.push('duplicate')
    if (n.root) c.push('root')
    return c.join(' ')
  }

  function subtitle(n: MapNode): string {
    const parts: string[] = []
    if (n.kind === 'screen') parts.push('screen')
    if (n.kind === 'compiled') parts.push('compiled')
    if (n.indirect) parts.push('indirect')
    if (n.says) parts.push(`${n.says} lines`)
    if (n.menus) parts.push(`${n.menus} menu${n.menus === 1 ? '' : 's'}`)
    if (n.returns) parts.push('return')
    if (n.dynamicOut) parts.push('dynamic')
    return parts.join(' · ')
  }

  function edgeWidth(count: number): number {
    return 1.1 + Math.min(2, Math.log2(count) * 0.5)
  }

  const prepared = $derived.by<Prepared | null>(() => {
    const l = layout
    if (!l) return null
    const data = new Map<string, MapNode>(map.nodes.map((n) => [n.id, n]))
    const hues = l.clusters.map((c) => clusterHue(c.key))
    const clusterOf = new Map<string, number>()
    for (const n of l.nodes) clusterOf.set(n.id, n.c)
    const clusters: DrawCluster[] = l.clusters.map((c, i) => {
      const hue = hues[i]
      return {
        i,
        key: c.key,
        x: c.x,
        y: c.y,
        w: c.w,
        h: c.h,
        count: c.count,
        fill: `hsl(${hue} var(--cluster-sat) var(--cluster-fill-l) / var(--cluster-fill-a))`,
        stroke: `hsl(${hue} var(--cluster-sat) var(--cluster-stroke-l) / var(--cluster-stroke-a))`,
        labelFill: `hsl(${hue} var(--cluster-sat) var(--cluster-label-l))`,
      }
    })
    const nodes: DrawNode[] = l.nodes.map((n) => {
      const d = data.get(n.id)
      const hue = hues[n.c] ?? 0
      const sub = d ? subtitle(d) : ''
      const titled = n.id.startsWith('screen:') ? n.id.slice(7) : n.id
      return {
        id: n.id,
        x: n.x,
        y: n.y,
        w: n.w,
        h: n.h,
        c: n.c,
        cls: nodeClass(d),
        tint:
          d?.kind === 'label' || d?.kind === 'compiled'
            ? `--tint:hsl(${hue} var(--cluster-sat) var(--cluster-tint-l) / var(--cluster-tint-a))`
            : undefined,
        title: truncate(titled, Math.floor((n.w - 16) / 7)),
        sub: sub ? truncate(sub, Math.floor((n.w - 16) / 6)) : '',
      }
    })
    const edges: DrawEdge[] = l.edges.map((e, i) => ({
      i,
      kind: e.kind,
      d: e.d,
      sw: edgeWidth(e.count),
      intra: e.intra,
      from: e.from,
      to: e.to,
      fromC: clusterOf.get(e.from) ?? -1,
      toC: clusterOf.get(e.to) ?? -1,
    }))
    const clusterEdges: DrawClusterEdge[] = l.clusterEdges.map((e, i) => ({
      i,
      from: e.from,
      to: e.to,
      d: e.d,
      sw: 1.2 + Math.min(4, Math.log2(e.weight + 1) * 0.6),
    }))
    return {
      nodes,
      clusters,
      edges,
      clusterEdges,
      nodeGrid: spatialIndex(nodes.map((n) => ({ x0: n.x, y0: n.y, x1: n.x + n.w, y1: n.y + n.h }))),
      clusterGrid: spatialIndex(clusters.map((c) => ({ x0: c.x, y0: c.y, x1: c.x + c.w, y1: c.y + c.h }))),
      edgeGrid: spatialIndex(indexBoxes(l.edges)),
      clusterEdgeGrid: spatialIndex(indexBoxes(l.clusterEdges)),
    }
  })

  function pick<T>(items: T[], index: SpatialIndex, x0: number, y0: number, x1: number, y1: number): T[] {
    return queryItems(items, index, x0, y0, x1, y1)
  }

  const edgesByNode = $derived.by(() => {
    const m = new Map<string, number[]>()
    ;(layout?.edges ?? []).forEach((e, i) => {
      for (const id of [e.from, e.to]) {
        const list = m.get(id)
        if (list) list.push(i)
        else m.set(id, [i])
      }
    })
    return m
  })

  const hotEdges = $derived(new Set(selected ? (edgesByNode.get(selected) ?? []) : []))
  const nearNodes = $derived.by(() => {
    const s = new Set<string>()
    if (!selected || !layout) return s
    for (const i of edgesByNode.get(selected) ?? []) {
      const e = layout.edges[i]
      s.add(e.from)
      s.add(e.to)
    }
    return s
  })

  function selectionPath(edges: DrawEdge[], hot: Set<number>): string {
    let d = ''
    for (const e of edges) {
      if (!hot.has(e.i)) continue
      d = d ? `${d} ${e.d}` : e.d
    }
    return d
  }

  interface VisibleNode extends DrawNode {
    /** 0 is a rectangle, 1 adds the title, 2 adds the subtitle. */
    tier: 0 | 1 | 2
  }

  interface Scene {
    clusters: DrawCluster[]
    /** Clusters that have at least one visible node. The rest stay boxes. */
    open: Set<number>
    nodes: VisibleNode[]
    edges: DrawEdge[]
    clusterEdges: DrawClusterEdge[]
    batch: boolean
    markers: boolean
  }

  /**
   * Every node on the screen shares one detail level. Zoom picks the level.
   * If the screen holds more nodes than the label budget, labels drop for all
   * of them and the rectangles stay. Past the rectangle budget the screen is
   * cluster boxes only, so no subset of peers is drawn alone. The cull rect is
   * padded by half the viewport; insetting a quarter of it is the screen.
   */
  function buildScene(v: ViewRect, P: Prepared): Scene {
    const clusters = pick(P.clusters, P.clusterGrid, v.x0, v.y0, v.x1, v.y1)
    const w = v.x1 - v.x0
    const h = v.y1 - v.y0
    const sx0 = v.x0 + w * 0.22
    const sy0 = v.y0 + h * 0.22
    const sx1 = v.x1 - w * 0.22
    const sy1 = v.y1 - h * 0.22
    const hits = pick(P.nodes, P.nodeGrid, sx0, sy0, sx1, sy1)
    const count = hits.length
    const show = v.k >= 0.14 && count <= NODE_RECT_BUDGET
    const tier: 0 | 1 | 2 =
      v.k >= 0.75 && count <= 160 ? 2 : v.k >= 0.35 && count <= NODE_BUDGET ? 1 : 0
    const nodes: VisibleNode[] = []
    const shown = new Set<string>()
    const open = new Set<number>()
    if (show) {
      for (const n of hits) {
        nodes.push({ ...n, tier })
        shown.add(n.id)
        open.add(n.c)
      }
    }

    const edges: DrawEdge[] = []
    if (shown.size) {
      for (const e of pick(P.edges, P.edgeGrid, sx0 - 10, sy0 - 10, sx1 + 10, sy1 + 10)) {
        if (shown.has(e.from) && shown.has(e.to)) edges.push(e)
      }
    }
    const clusterEdges =
      v.k >= 0.4
        ? []
        : pick(P.clusterEdges, P.clusterEdgeGrid, v.x0, v.y0, v.x1, v.y1).filter(
            (e) => !(open.has(e.from) && open.has(e.to)),
          )
    const batch = !show || tier < 1 || edges.length > DETAIL_EDGES
    return { clusters, open, nodes, edges, clusterEdges, batch, markers: !batch }
  }

  const layoutById = $derived(new Map((layout?.nodes ?? []).map((n) => [n.id, n])))

  // Bring the selected node into view when it was chosen elsewhere (sidebar, code).
  $effect(() => {
    const id = selected
    const l = layoutById
    if (!id || !pz) return
    const n = l.get(id)
    if (!n) return
    untrack(() => {
      const v = pz!.currentRect()
      if (!intersects(v, n.x, n.y, n.x + n.w, n.y + n.h, -30)) {
        pz!.centerOn(n.x + n.w / 2, n.y + n.h / 2, Math.max(v.k, 0.5))
      }
    })
  })

  // Drag from a scene to another one to wire them together.
  interface Link {
    from: string
    sx: number
    sy: number
    x: number
    y: number
    hover: string | null
  }
  interface LinkMenu {
    x: number
    y: number
    from: string
    to: string
  }
  let mapEl: HTMLDivElement | undefined = $state()
  let link = $state<Link | null>(null)
  let linkMenu = $state<LinkMenu | null>(null)

  /** Only scenes in editable script files can get a new connection. */
  function canLink(id: string): boolean {
    const n = nodeByName(id)
    if (!n || n.kind !== 'label') return false
    const file = fileOfNode(n)
    return !!file && fileInfo(file)?.editable !== false
  }

  function nodeAt(P: Prepared, x: number, y: number, skip: string): DrawNode | null {
    for (const i of P.nodeGrid.query(x, y, x, y)) {
      const n = P.nodes[i]
      if (n.id === skip) continue
      if (x >= n.x && x <= n.x + n.w && y >= n.y && y <= n.y + n.h) return n
    }
    return null
  }

  function canTarget(id: string): boolean {
    const k = nodeByName(id)?.kind
    return k === 'label' || k === 'compiled'
  }

  function startLink(n: DrawNode, e: PointerEvent) {
    e.stopPropagation()
    e.preventDefault()
    if (e.button !== 0 || !pz) return
    const sx = n.x + n.w
    const sy = n.y + n.h / 2
    linkMenu = null
    link = { from: n.id, sx, sy, x: sx, y: sy, hover: null }
    const move = (ev: PointerEvent) => {
      if (!link || !pz || !prepared) return
      const pt = pz.toContent(ev.clientX, ev.clientY)
      const hit = nodeAt(prepared, pt.x, pt.y, link.from)
      link = { ...link, x: pt.x, y: pt.y, hover: hit && canTarget(hit.id) ? hit.id : null }
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
      link = null
    }
    const stop = (ev: PointerEvent) => {
      cleanup()
      const l = link
      link = null
      if (!l || !mapEl) return
      if (l.hover) {
        const box = mapEl.getBoundingClientRect()
        linkMenu = { x: ev.clientX - box.left, y: ev.clientY - box.top, from: l.from, to: l.hover }
      }
    }
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', stop)
    window.addEventListener('keydown', cancel, true)
  }

  async function connect(how: 'jump' | 'call' | 'choice') {
    const m = linkMenu
    linkMenu = null
    if (!m) return
    const from = nodeByName(m.from)
    const path = from ? fileOfNode(from) : null
    if (!from || !path) return
    let caption: string | undefined
    if (how === 'choice') {
      const got = await askText(`Choice text that leads to ${m.to}`, '', 'Add choice')
      if (got === null || !got.trim()) return
      caption = got.trim()
    }
    await sceneEdit({ op: 'link-scene', path, line: from.line, how, target: m.to, text: caption })
  }
</script>

<div class="map" bind:this={mapEl}>
  {#if layout && prepared}
    {@const P = prepared}
    <PanZoom
      bind:this={pz}
      contentWidth={layout.width}
      contentHeight={layout.height}
      fadeKey={layoutGen}
      {fitKey}
    >
      {#snippet children(v)}
        {@const S = buildScene(v, P)}
        {@const labelPx = Math.min(90, Math.max(14, 13 / v.k))}
        {@const labelY = Math.min(90, 13 / v.k)}
        {@const batches = S.batch ? batchPaths(S.edges, () => false) : []}
        {@const hotD = selectionPath(S.edges, hotEdges)}
        <defs>
          {#each ['jump', 'choice', 'fall', 'call', 'screen', 'action'] as kind (kind)}
            <marker
              id={`arrow-${kind}`}
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

        {#each S.clusters as c (c.key)}
          <rect
            x={c.x}
            y={c.y}
            width={c.w}
            height={c.h}
            rx="14"
            style={`fill:${c.fill};stroke:${c.stroke}`}
            class="cluster"
          />
          <text
            x={c.x + 14}
            y={c.y + (S.open.has(c.i) ? 22 : labelY)}
            class="cluster-label"
            style={`font-size:${S.open.has(c.i) ? 16 : labelPx}px;fill:${c.labelFill}`}
          >
            {c.key} <tspan class="cluster-count">· {c.count}</tspan>
          </text>
        {/each}

        {#each S.clusterEdges as e (e.i)}
          <path d={e.d} class="cluster-edge" stroke-width={e.sw / v.k} />
        {/each}

        <g class="edges" class:dim={selected !== null}>
          {#if S.batch}
            {#each batches as b (b.kind)}
              <path d={b.d} class={`edge ${b.kind} flat`} />
            {/each}
          {:else}
            {#each S.edges as e (e.i)}
              <path
                d={e.d}
                class={`edge ${e.kind}`}
                class:inter={!e.intra}
                style={`stroke-width:${e.sw}px`}
                marker-end={S.markers ? `url(#arrow-${e.kind})` : undefined}
              />
            {/each}
          {/if}
        </g>
        {#if hotD}
          <path d={hotD} class="edge hot" />
        {/if}

        {#each S.nodes as n (n.id)}
          <g
            class={`node ${n.cls}`}
            class:selected={selected === n.id}
            class:near={selected !== n.id && nearNodes.has(n.id)}
            class:faded={selected !== null && selected !== n.id && !nearNodes.has(n.id)}
            class:drop={link !== null && link.hover === n.id}
            role="button"
            tabindex="-1"
            onclick={() => onselect(n.id)}
            ondblclick={() => onopen(n.id)}
            oncontextmenu={(e) => {
              app.selectedLabel = n.id
              openContextMenu(e, labelItems(n.id))
            }}
            onkeydown={(e) => e.key === 'Enter' && onopen(n.id)}
          >
            <rect x={n.x} y={n.y} width={n.w} height={n.h} rx="7" style={n.tint} />
            {#if n.tier >= 1}
              <text x={n.x + 10} y={n.y + (n.tier >= 2 && n.sub ? 16 : n.h / 2 + 4)} class="title">
                {n.title}
                <title>{n.id}</title>
              </text>
              {#if n.tier >= 2 && n.sub}
                <text x={n.x + 10} y={n.y + 30} class="sub">{n.sub}</text>
              {/if}
              {#if canLink(n.id)}
                <!-- svelte-ignore a11y_click_events_have_key_events -->
                <g
                  class="handle"
                  role="button"
                  tabindex="-1"
                  onpointerdown={(e) => startLink(n, e)}
                  onclick={(e) => e.stopPropagation()}
                  ondblclick={(e) => e.stopPropagation()}
                >
                  <title>Drag onto another scene to connect</title>
                  <circle cx={n.x + n.w} cy={n.y + n.h / 2} r={Math.min(14, 6 / v.k)} />
                </g>
              {/if}
            {/if}
          </g>
        {/each}
        {#if link}
          <path
            d={`M${link.sx} ${link.sy} L${link.x} ${link.y}`}
            class="link-line"
            style={`stroke-width:${2 / v.k}px;stroke-dasharray:${6 / v.k} ${4 / v.k}`}
          />
        {/if}
      {/snippet}
      {#snippet hud(v)}
        <Hud>
          <ZoomControls k={v.k} onfit={() => pz?.fit()} onin={() => pz?.zoomBy(1.4)} onout={() => pz?.zoomBy(1 / 1.4)} />
          <select bind:value={mode} title="Group nodes by">
            <option value="prefix">Group: name prefix</option>
            <option value="file">Group: file</option>
          </select>
          <span class="info">{map.nodes.length} nodes · {map.edges.length} edges</span>
        </Hud>
        <div class="legend">
          <span><i class="sw label"></i>label</span>
          <span><i class="sw kmenu"></i>named menu</span>
          <span><i class="sw screen"></i>screen</span>
          <span><i class="sw compiled"></i>compiled only</span>
          <span><i class="sw missing"></i>missing target</span>
          <span><i class="sw unreachable"></i>unreachable</span>
          <span><i class="ln jump"></i>jump</span>
          <span><i class="ln choice"></i>menu choice</span>
          <span><i class="ln fall"></i>falls through</span>
          <span><i class="ln call"></i>call</span>
          <span><i class="ln screen"></i>opens screen</span>
          <span><i class="ln action"></i>screen button</span>
        </div>
      {/snippet}
    </PanZoom>
  {/if}
  {#if linkMenu}
    {@const m = linkMenu}
    <div class="link-scrim" role="presentation" onpointerdown={() => (linkMenu = null)}></div>
    <div
      class="link-menu menu anim-pop"
      role="menu"
      tabindex="-1"
      style={`left:${Math.max(4, m.x)}px;top:${Math.max(4, m.y)}px`}
      onkeydown={(e) => e.key === 'Escape' && (linkMenu = null)}
    >
      <button role="menuitem" class="menu-item" onclick={() => void connect('jump')}>Jump to {m.to} at the end of {m.from}</button>
      <button role="menuitem" class="menu-item" onclick={() => void connect('call')}>Call {m.to} from {m.from}</button>
      <button role="menuitem" class="menu-item" onclick={() => void connect('choice')}>Add a choice to {m.to} in {m.from}…</button>
    </div>
  {/if}
  {#if !loading && !error && !map.nodes.length}
    <div class="empty-state map-empty">
      <Icon name="map" size={28} />
      <p>No labels or screens to map yet.</p>
      <p class="hint">Add a <code>label</code> to a script and the map fills in.</p>
    </div>
  {/if}
  {#if loading}
    <div class="spinner-overlay veil" role="status"><div class="spinner"></div>{progress}</div>
  {/if}
  {#if error}
    <div class="spinner-overlay veil" role="alert">
      <div class="alert err card">
        <Icon name="error" size={14} />
        <span class="msg">Layout failed: {error}</span>
        <button type="button" class="sm" onclick={() => (attempt += 1)}>Retry</button>
      </div>
    </div>
  {/if}
</div>

<style>
  .map {
    position: relative;
    width: 100%;
    height: 100%;
  }
  .veil {
    background: color-mix(in srgb, var(--bg-canvas) 70%, transparent);
    font-size: var(--fs-lg);
  }
  .map-empty {
    position: absolute;
    inset: 0;
    justify-content: center;
    pointer-events: none;
  }
  .map-empty .hint {
    font-size: var(--fs-sm);
  }
  .legend {
    position: absolute;
    left: 10px;
    bottom: 10px;
    display: flex;
    gap: 12px;
    flex-wrap: wrap;
    font-size: var(--fs-sm);
    color: var(--dim);
    background: color-mix(in srgb, var(--panel) 92%, transparent);
    border: 1px solid var(--line);
    border-radius: var(--r-lg);
    box-shadow: var(--shadow-sm);
    padding: 6px 10px;
    max-width: 90%;
  }
  .legend span {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
  .sw {
    width: 12px;
    height: 12px;
    border-radius: var(--r-sm);
    border: 1.5px solid var(--node-stroke);
    background: var(--node-fill);
  }
  .sw.kmenu {
    border-color: var(--menu);
  }
  .sw.screen {
    border-color: var(--screen);
  }
  .sw.compiled {
    border-style: dotted;
    border-color: var(--fall);
  }
  .sw.missing {
    border-color: var(--error);
    background: color-mix(in srgb, var(--error) 25%, var(--node-fill));
  }
  .sw.unreachable {
    border-style: dashed;
    border-color: var(--dim);
  }
  .ln {
    width: 18px;
    height: 0;
    border-top: 2px solid var(--edge);
  }
  .ln.choice {
    border-color: var(--choice);
  }
  .ln.fall {
    border-top-style: dashed;
    border-color: var(--fall);
  }
  .ln.screen {
    border-color: var(--screen);
  }
  .ln.action {
    border-top-style: dashed;
    border-color: var(--action);
  }
  .ln.call {
    border-top-style: dotted;
    border-color: var(--call);
  }

  .cluster {
    stroke-width: 1.2;
  }
  .cluster-label {
    font-weight: 600;
    pointer-events: none;
  }
  .cluster-count {
    font-weight: 400;
    opacity: 0.6;
  }
  .cluster-edge {
    fill: none;
    stroke: var(--edge);
    opacity: 0.45;
    pointer-events: none;
  }
  .edge {
    fill: none;
    stroke: var(--edge);
    stroke-width: 1.4;
    opacity: 0.75;
    pointer-events: none;
  }
  .edge.inter {
    opacity: 0.45;
  }
  .edge.choice {
    stroke: var(--choice);
  }
  .edge.fall {
    stroke: var(--fall);
    stroke-dasharray: 6 4;
  }
  .edge.screen {
    stroke: var(--screen);
  }
  .edge.action {
    stroke: var(--action);
    stroke-dasharray: 5 3;
  }
  .edge.call {
    stroke: var(--call);
    stroke-dasharray: 2 3;
  }
  .edge.flat {
    stroke-dasharray: none;
  }
  .edges.dim .edge {
    opacity: 0.12;
  }
  .edge.hot {
    opacity: 1;
    stroke-width: 2.4px !important;
    stroke: var(--accent);
  }
  .arrow {
    fill: var(--edge);
  }
  .arrow.choice {
    fill: var(--choice);
  }
  .arrow.fall {
    fill: var(--fall);
  }
  .arrow.screen {
    fill: var(--screen);
  }
  .arrow.action {
    fill: var(--action);
  }
  .arrow.call {
    fill: var(--call);
  }

  .node {
    cursor: pointer;
  }
  .node rect {
    fill: var(--tint, var(--node-fill));
    stroke: var(--node-stroke);
    stroke-width: 1.4;
  }
  .node:hover rect {
    stroke: var(--accent);
  }
  .node.menu rect {
    stroke: var(--menu);
  }
  .node.screen rect {
    stroke: var(--screen);
    fill: color-mix(in srgb, var(--screen) 14%, var(--node-fill));
  }
  .node.compiled rect {
    stroke-dasharray: 2 3;
    stroke: var(--fall);
  }
  .node.missing rect {
    stroke: var(--error);
    fill: color-mix(in srgb, var(--error) 22%, var(--node-fill));
  }
  .node.unreachable rect {
    stroke-dasharray: 5 3;
    stroke: var(--dim);
  }
  .node.duplicate rect {
    stroke: var(--warning);
  }
  .node.root rect {
    stroke: var(--ok);
    stroke-width: 2.4;
  }
  .node.near rect {
    stroke: var(--accent);
  }
  .node.selected rect {
    stroke: var(--accent);
    stroke-width: 3;
  }
  .node.faded {
    opacity: 0.35;
  }
  .node.drop rect {
    stroke: var(--ok);
    stroke-width: 3;
  }
  .handle {
    cursor: crosshair;
    opacity: 0;
  }
  .handle circle {
    fill: var(--accent);
    stroke: var(--bg-canvas);
    stroke-width: 1.5;
  }
  .node:hover .handle,
  .node.selected .handle {
    opacity: 1;
  }
  .link-line {
    fill: none;
    stroke: var(--accent);
    pointer-events: none;
  }
  .link-scrim {
    position: absolute;
    inset: 0;
  }
  .link-menu {
    position: absolute;
    z-index: var(--z-split);
  }
  .link-menu :global(.menu-item) {
    white-space: nowrap;
  }
  .title {
    fill: var(--text);
    font-size: var(--fs-lg);
    font-weight: 600;
    pointer-events: none;
  }
  .sub {
    fill: var(--dim);
    font-size: var(--fs-xs);
    pointer-events: none;
  }
</style>
