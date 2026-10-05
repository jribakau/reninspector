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
  import { DETAIL_EDGES, batchPaths, indexBoxes, intersects, queryItems, spatialIndex, truncate, type SpatialIndex, type ViewRect } from '../lib/view'
  import { fitChars, hairline, mapLevel, markRect, markState, nearestBox, titleFont } from '../lib/graph'
  import { HELPERS_KEY, groupTitle } from '../lib/mapgroups'
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

  let mode = $state<ClusterMode>('auto')
  /** Shared subroutines sit in a lane of their own. Their links show for the selected scene only. */
  let helpersOn = $state(localStorage.getItem('vnide.mapHelpers') !== '0')

  function toggleHelpers() {
    helpersOn = !helpersOn
    localStorage.setItem('vnide.mapHelpers', helpersOn ? '1' : '0')
  }
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

  const EDGE_FILTERS = [
    { kind: 'jump', name: 'jump' },
    { kind: 'choice', name: 'menu choice' },
    { kind: 'fall', name: 'falls through' },
    { kind: 'call', name: 'call' },
    { kind: 'screen', name: 'opens screen' },
    { kind: 'action', name: 'screen button' },
  ] as const

  function loadHiddenKinds(): string[] {
    try {
      const raw = localStorage.getItem('vnide.mapHiddenEdges')
      if (!raw) return []
      const parsed = JSON.parse(raw) as unknown
      if (!Array.isArray(parsed)) return []
      const known = new Set<string>(EDGE_FILTERS.map((e) => e.kind))
      return parsed.filter((k): k is string => typeof k === 'string' && known.has(k))
    } catch {
      return []
    }
  }

  let legendOpen = $state(localStorage.getItem('vnide.mapLegend') !== '0')
  let hiddenKinds = $state<string[]>(loadHiddenKinds())

  function toggleLegend() {
    legendOpen = !legendOpen
    localStorage.setItem('vnide.mapLegend', legendOpen ? '1' : '0')
  }

  function kindHidden(kind: string): boolean {
    return hiddenKinds.includes(kind)
  }

  function toggleKind(kind: string) {
    hiddenKinds = kindHidden(kind) ? hiddenKinds.filter((k) => k !== kind) : [...hiddenKinds, kind]
    localStorage.setItem('vnide.mapHiddenEdges', JSON.stringify(hiddenKinds))
  }

  const fitKey = $derived(`${root}|${mode}|${helpersOn}|${map.nodes.length}|${map.edges.length}`)

  $effect(() => {
    const m = map
    const md = mode
    const hp = helpersOn
    const r = root
    const f = files
    const key = mapCacheKey(r, m, md, hp)
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
    void layoutProjectMap(
      m,
      f,
      md,
      key,
      (done, total) => {
        if (t === token) progress = `Laying out clusters ${done}/${total}`
      },
      { helpers: hp },
    )
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
    root: boolean
    ends: boolean
    returns: boolean
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
    start: boolean
  }

  interface DrawEdge {
    i: number
    kind: string
    /** Full route, including the shared trunk. Used to trace a selected connection. */
    d: string
    /** What the normal layer draws. Fans for a cross-group edge, the full route inside a group. */
    draw: string
    sw: number
    intra: boolean
    from: string
    to: string
    fromC: number
    toC: number
    /** A link into a side group. Drawn only for the selected scene. */
    helper: boolean
  }

  interface DrawClusterEdge {
    i: number
    from: number
    to: number
    d: string
    sw: number
  }

  /** Every node of one look, as a single path. Drawn when there is no room for the nodes themselves. */
  interface Mark {
    key: string
    cls: string
    style: string | undefined
    d: string
  }

  interface Prepared {
    marks: Mark[]
    roots: DrawNode[]
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
    const starts = new Set<number>()
    for (const n of l.nodes) {
      clusterOf.set(n.id, n.c)
      if (data.get(n.id)?.root) starts.add(n.c)
    }
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
        start: starts.has(i),
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
        title: titled,
        sub: sub ? truncate(sub, Math.floor((n.w - 20) / 6)) : '',
        root: !!d?.root,
        ends: !!d?.endsScript,
        returns: !!d?.returns,
      }
    })
    const edges: DrawEdge[] = l.edges.map((e, i) => ({
      i,
      kind: e.kind,
      d: e.d,
      draw: e.intra ? e.d : e.fan || e.d,
      sw: edgeWidth(e.count),
      intra: e.intra,
      from: e.from,
      to: e.to,
      fromC: clusterOf.get(e.from) ?? -1,
      toC: clusterOf.get(e.to) ?? -1,
      helper: !!e.helper,
    }))
    const clusterEdges: DrawClusterEdge[] = l.clusterEdges.map((e, i) => ({
      i,
      from: e.from,
      to: e.to,
      d: e.d,
      sw: 1.2 + Math.min(4, Math.log2(e.weight + 1) * 0.6),
    }))
    // One path per look: a group's colour for scenes, a fixed colour for the rest.
    const buckets = new Map<string, Mark>()
    for (const n of nodes) {
      const state = markState(data.get(n.id))
      const tinted = state === 'label' || state === 'compiled'
      const key = tinted ? `${state}|${n.c}` : state
      let m = buckets.get(key)
      if (!m) {
        const fill = clusters[n.c]?.labelFill
        m = { key, cls: tinted ? 'scene' : state, style: tinted && fill ? `fill:${fill};stroke:${fill}` : undefined, d: '' }
        buckets.set(key, m)
      }
      m.d += markRect(n.x, n.y, n.w, n.h)
    }
    return {
      // Start scenes last, so they sit on top.
      marks: [...buckets.values()].sort((a, b) => Number(a.cls === 'root') - Number(b.cls === 'root')),
      roots: nodes.filter((n) => n.root),
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

  const hiddenSet = $derived(new Set(hiddenKinds))
  const helperCount = $derived(layout?.clusters.find((c) => c.key === HELPERS_KEY)?.count ?? 0)

  /** Group pairs that still have a visible link. Recomputed only when the map or the filter changes, not on every pan. */
  const linkedPairs = $derived.by(() => {
    const out = new Set<string>()
    for (const e of prepared?.edges ?? []) {
      if (e.intra || e.helper || hiddenSet.has(e.kind)) continue
      out.add(`${e.fromC}>${e.toC}`)
    }
    return out
  })

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
    hot: DrawEdge[]
    batch: boolean
    markers: boolean
    /** No room for the nodes themselves, so every node is drawn as a minimum-size mark. */
    marks: boolean
  }

  /**
   * Every node on the screen shares one detail level. Zoom picks the level.
   * If the screen holds more nodes than the label budget, labels drop for all
   * of them and the rectangles stay. Past the rectangle budget the screen is
   * cluster boxes only, so no subset of peers is drawn alone. The cull rect is
   * padded by half the viewport; insetting a quarter of it is the screen.
   */
  function buildScene(v: ViewRect, P: Prepared): Scene {
    const hidden = hiddenSet
    const clusters = pick(P.clusters, P.clusterGrid, v.x0, v.y0, v.x1, v.y1)
    const w = v.x1 - v.x0
    const h = v.y1 - v.y0
    const sx0 = v.x0 + w * 0.22
    const sy0 = v.y0 + h * 0.22
    const sx1 = v.x1 - w * 0.22
    const sy1 = v.y1 - h * 0.22
    const hits = pick(P.nodes, P.nodeGrid, sx0, sy0, sx1, sy1)
    const { show, tier } = mapLevel(v.k, hits.length)
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

    const pairs = linkedPairs
    const edges: DrawEdge[] = []
    if (shown.size) {
      for (const e of pick(P.edges, P.edgeGrid, sx0 - 10, sy0 - 10, sx1 + 10, sy1 + 10)) {
        if (e.helper || hidden.has(e.kind)) continue
        if (shown.has(e.from) && shown.has(e.to)) edges.push(e)
      }
    }
    const clusterEdges = pick(P.clusterEdges, P.clusterEdgeGrid, v.x0, v.y0, v.x1, v.y1).filter((e) =>
      pairs.has(`${e.from}>${e.to}`),
    )
    // Links into the helper lane are not drawn until a scene is selected, so they come from the selection, not from what is on screen.
    const hot: DrawEdge[] = []
    for (const i of hotEdges) {
      const e = P.edges[i]
      if (!e || hidden.has(e.kind)) continue
      if (e.helper || (shown.has(e.from) && shown.has(e.to))) hot.push(e)
    }
    const batch = !show || tier < 1 || edges.length > DETAIL_EDGES
    return { clusters, open, nodes, edges, clusterEdges, hot, batch, markers: !batch, marks: !show }
  }

  /** The selected scene and the ones it links to, as paths for the mark layer. Independent of the view. */
  const markHighlight = $derived.by(() => {
    if (!selected) return null
    const rect = (id: string) => {
      const n = layoutById.get(id)
      return n ? markRect(n.x, n.y, n.w, n.h) : ''
    }
    let near = ''
    for (const id of nearNodes) if (id !== selected) near += rect(id)
    return { selected: rect(selected), near }
  })

  /** The node nearest a click on the mark layer. The marks are fatter than the nodes, so the reach is the mark's half width. */
  function markAt(P: Prepared, e: MouseEvent, k: number): DrawNode | null {
    if (!pz) return null
    const pt = pz.toContent(e.clientX, e.clientY)
    const reach = 2 / Math.max(k, 0.001)
    const near = P.nodeGrid.query(pt.x - reach, pt.y - reach, pt.x + reach, pt.y + reach).map((i) => P.nodes[i])
    return nearestBox(near, pt.x, pt.y, reach)
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
        {@const batches = S.batch ? batchPaths(S.edges.map((e) => ({ kind: e.kind, d: e.draw })), () => false) : []}
        {@const hair = hairline(v.k)}
        {@const font = titleFont(v.k)}
        {@const tag = Math.min(2.4, Math.max(1, 0.85 / v.k))}
        {@const landmark = Math.min(48, 12 / v.k)}
        <g class="scene" style={`--hair:${hair}`}>
        <defs>
          {#each ['jump', 'choice', 'fall', 'call', 'screen', 'action', 'trunk'] as kind (kind)}
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
          {@const open = S.open.has(c.i)}
          {@const fs = open ? 16 : labelPx}
          {@const ty = c.y + (open ? 22 : labelY)}
          {@const play = c.start ? fs * 0.55 : 0}
          <rect
            x={c.x}
            y={c.y}
            width={c.w}
            height={c.h}
            rx="14"
            style={`fill:${c.fill};stroke:${c.stroke}`}
            class="cluster"
          />
          {#if c.start}
            <polygon
              class="play"
              points={`${c.x + 12},${ty - fs * 0.72} ${c.x + 12 + play},${ty - fs * 0.36} ${c.x + 12},${ty - 1}`}
            />
          {/if}
          <text
            x={c.x + 14 + (c.start ? play + 4 : 0)}
            y={ty}
            class="cluster-label"
            style={`font-size:${fs}px;fill:${c.labelFill}`}
          >
            {groupTitle(c.key)} <tspan class="cluster-count">· {c.count}</tspan>
          </text>
        {/each}

        <g class="trunks" class:dim={selected !== null}>
          {#each S.clusterEdges as e (e.i)}
            <path
              d={e.d}
              class="cluster-edge"
              stroke-width={Math.max(e.sw, 1.2 / v.k)}
              marker-end="url(#arrow-trunk)"
            />
          {/each}
        </g>

        <g class="edges" class:dim={selected !== null}>
          {#if S.batch}
            {#each batches as b (b.kind)}
              <path d={b.d} class={`edge ${b.kind} flat`} />
            {/each}
          {:else}
            {#each S.edges as e (e.i)}
              <path
                d={e.draw}
                class={`edge ${e.kind}`}
                class:inter={!e.intra}
                style={`stroke-width:${Math.max(e.sw, 0.9 / v.k)}px`}
                marker-end={S.markers ? `url(#arrow-${e.kind})` : undefined}
              />
            {/each}
          {/if}
        </g>
        {#each S.hot as e (e.i)}
          <path d={e.d} class={`edge hot ${e.kind}`} marker-end={`url(#arrow-${e.kind})`} />
        {/each}

        {#if S.marks}
          <!-- Too far out, or too crowded, for the nodes themselves. Each one is still drawn, as a mark at least 3px across. -->
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <g
            class="marks"
            class:dim={selected !== null}
            role="presentation"
            onclick={(e) => {
              const n = markAt(P, e, v.k)
              if (n) onselect(n.id)
            }}
            ondblclick={(e) => {
              const n = markAt(P, e, v.k)
              if (n) onopen(n.id)
            }}
            oncontextmenu={(e) => {
              const n = markAt(P, e, v.k)
              if (!n) return
              app.selectedLabel = n.id
              openContextMenu(e, labelItems(n.id))
            }}
          >
            {#each P.marks as m (m.key)}
              <path d={m.d} class={`mark ${m.cls}`} style={m.style} />
            {/each}
            {#if markHighlight}
              <path d={markHighlight.near} class="mark near" />
              <path d={markHighlight.selected} class="mark selected" />
            {/if}
          </g>
          {#each P.roots as n (n.id)}
            {#if intersects(v, n.x, n.y, n.x + n.w, n.y + n.h, 0)}
              <text x={n.x} y={n.y - 6 / v.k} class="landmark" style={`font-size:${landmark}px`}>
                {truncate(n.title, 28)}
              </text>
            {/if}
          {/each}
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
            <rect x={n.x} y={n.y} width={n.w} height={n.h} rx="8" style={n.tint} />
            {#if n.tier === 0 && n.root}
              <text x={n.x} y={n.y - 6 / v.k} class="landmark" style={`font-size:${landmark}px`}>
                {truncate(n.title, 28)}
              </text>
            {/if}
            {#if n.tier >= 1 && n.root}
              <g class="badge start" transform={`translate(${n.x} ${n.y}) scale(${tag})`}>
                <rect x="0" y="-13" width="42" height="13" rx="4" />
                <text x="21" y="-3" text-anchor="middle">START</text>
              </g>
            {/if}
            {#if n.tier >= 1 && n.ends}
              <g class="badge end" transform={`translate(${n.x + n.w} ${n.y}) scale(${tag})`}>
                <rect x="-32" y="-13" width="32" height="13" rx="4" />
                <text x="-16" y="-3" text-anchor="middle">END</text>
              </g>
            {/if}
            {#if n.tier >= 1}
              {@const room = n.returns ? 34 : 20}
              {#if n.returns}
                <text x={n.x + n.w - 10} y={n.y + 18} text-anchor="end" class="ret" style={`font-size:${Math.max(12, font * 0.9)}px`}>↩</text>
              {/if}
              <text
                x={n.x + 12}
                y={n.tier >= 2 && n.sub ? n.y + 20 : n.y + n.h / 2 + font * 0.34}
                class="title"
                style={`font-size:${font}px`}
              >
                {truncate(n.title, fitChars(n.w, font, room))}
                <title>{n.id}</title>
              </text>
              {#if n.tier >= 2 && n.sub}
                <text x={n.x + 12} y={n.y + 36} class="sub">{n.sub}</text>
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
        </g>
      {/snippet}
      {#snippet hud(v)}
        <Hud>
          <ZoomControls k={v.k} onfit={() => pz?.fit()} onin={() => pz?.zoomBy(1.4)} onout={() => pz?.zoomBy(1 / 1.4)} />
          <select bind:value={mode} title="Group nodes by">
            <option value="auto">Group: automatic</option>
            <option value="flow">Group: story flow</option>
            <option value="prefix">Group: name prefix</option>
            <option value="file">Group: file</option>
          </select>
          <button
            class:on={helpersOn}
            aria-pressed={helpersOn}
            onclick={toggleHelpers}
            title="Move shared subroutines into their own box. Their links show when you select a scene."
          >
            Helpers
          </button>
          <span class="info">
            {map.nodes.length} nodes · {map.edges.length} edges{helperCount ? ` · ${helperCount} helpers` : ''}
          </span>
          <button class:on={legendOpen} aria-pressed={legendOpen} onclick={toggleLegend} title="Show what the colors and marks mean">Legend</button>
        </Hud>
        {#if legendOpen}
          <div class="legend" role="note" aria-label="Legend">
            <div class="lg-group">
              <span class="lg-title">Scenes</span>
              <span class="lg-item"><i class="sw label"></i>label (fill = group)</span>
              <span class="lg-item"><i class="sw kmenu"></i>named menu</span>
              <span class="lg-item"><i class="sw screen"></i>screen</span>
              <span class="lg-item"><i class="sw compiled"></i>compiled only</span>
              <span class="lg-item"><i class="sw missing"></i>missing target</span>
              <span class="lg-item"><i class="sw unreachable"></i>unreachable</span>
              <span class="lg-item"><i class="sw duplicate"></i>duplicate</span>
            </div>
            <div class="lg-group">
              <span class="lg-title">Connections</span>
              {#each EDGE_FILTERS as item (item.kind)}
                <button
                  type="button"
                  class="lg-btn"
                  class:off={kindHidden(item.kind)}
                  aria-pressed={!kindHidden(item.kind)}
                  title={kindHidden(item.kind) ? `Show ${item.name}` : `Hide ${item.name}`}
                  onclick={() => toggleKind(item.kind)}
                >
                  <svg class={`ln ${item.kind}`} viewBox="0 0 22 8" aria-hidden="true">
                    <line x1="1" y1="4" x2="21" y2="4" />
                  </svg>
                  {item.name}
                </button>
              {/each}
            </div>
            <div class="lg-group">
              <span class="lg-title">Markers</span>
              <span class="lg-item"><i class="tag-sw start">START</i>start</span>
              <span class="lg-item"><i class="tag-sw end">END</i>end</span>
              <span class="lg-item"><i class="sw selected"></i>selected</span>
              <span class="lg-item"><i class="sw near"></i>connected to selection</span>
              <span class="lg-note">Thicker lines mean more links</span>
              {#if helperCount}
                <span class="lg-note">
                  Helpers are scenes other scenes call and return from. Select a scene to see which it uses.
                </span>
              {/if}
            </div>
          </div>
        {/if}
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
    z-index: var(--z-raised);
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: var(--fs-sm);
    color: var(--dim);
    background: color-mix(in srgb, var(--panel) 92%, transparent);
    border: 1px solid var(--line);
    border-radius: var(--r-lg);
    box-shadow: var(--shadow-sm);
    padding: 8px 10px;
    max-width: calc(100% - 44px);
  }
  .lg-group {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 12px;
  }
  .lg-title {
    font-weight: 600;
    color: var(--text);
    min-width: 88px;
  }
  .lg-item,
  .lg-note {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
  .lg-btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 0;
    background: none;
    border: none;
    border-radius: 0;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }
  .lg-btn:hover {
    color: var(--text);
    border: none;
  }
  .lg-btn.off {
    text-decoration: line-through;
    opacity: 0.5;
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
  .sw.duplicate {
    border-color: var(--warning);
  }
  .sw.selected {
    border-color: var(--accent);
    border-width: 2.5px;
  }
  .sw.near {
    border-color: var(--accent);
  }
  .tag-sw {
    font-style: normal;
    font-size: 8px;
    font-weight: 700;
    letter-spacing: 0.04em;
    padding: 1px 3px;
    border-radius: 3px;
    line-height: 1.2;
  }
  .tag-sw.start {
    background: var(--ok);
    color: var(--on-accent);
  }
  .tag-sw.end {
    color: var(--dim);
    border: 1px solid var(--dim);
  }
  .ln {
    width: 22px;
    height: 8px;
    flex: none;
  }
  .ln line {
    stroke: var(--edge);
    stroke-width: 2;
  }
  .ln.choice line {
    stroke: var(--choice);
  }
  .ln.fall line {
    stroke: var(--fall);
    stroke-dasharray: 6 4;
  }
  .ln.screen line {
    stroke: var(--screen);
  }
  .ln.action line {
    stroke: var(--action);
    stroke-dasharray: 5 3;
  }
  .ln.call line {
    stroke: var(--call);
    stroke-dasharray: 2 3;
  }

  .cluster {
    stroke-width: calc(1.2px * var(--hair, 1));
  }
  .landmark {
    fill: var(--ok);
    font-weight: 700;
    paint-order: stroke;
    stroke: var(--bg-canvas);
    stroke-width: 0.25em;
    stroke-linejoin: round;
    pointer-events: none;
  }
  .cluster-label {
    font-weight: 600;
    pointer-events: none;
  }
  .cluster-count {
    font-weight: 400;
    opacity: 0.6;
  }
  .play {
    fill: var(--ok);
    pointer-events: none;
  }
  .trunks.dim {
    opacity: 0.2;
  }
  .cluster-edge {
    fill: none;
    stroke: var(--edge);
    opacity: 0.7;
    pointer-events: none;
  }
  .edge {
    fill: none;
    stroke: var(--edge);
    stroke-width: calc(1.4px * var(--hair, 1));
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
    stroke-width: calc(2.4px * var(--hair, 1));
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
  .node > rect {
    fill: var(--tint, var(--node-fill));
    stroke: var(--node-stroke);
    stroke-width: calc(1.4px * var(--hair, 1));
  }
  .node:hover > rect {
    stroke: var(--accent);
  }
  .node.menu > rect {
    stroke: var(--menu);
  }
  .node.screen > rect {
    stroke: var(--screen);
    fill: color-mix(in srgb, var(--screen) 14%, var(--node-fill));
  }
  .node.compiled > rect {
    stroke-dasharray: 2 3;
    stroke: var(--fall);
  }
  .node.missing > rect {
    stroke: var(--error);
    fill: color-mix(in srgb, var(--error) 22%, var(--node-fill));
  }
  .node.unreachable > rect {
    stroke-dasharray: 5 3;
    stroke: var(--dim);
  }
  .node.duplicate > rect {
    stroke: var(--warning);
  }
  .node.root > rect {
    stroke: var(--ok);
    stroke-width: calc(2.4px * var(--hair, 1));
  }
  .node.near > rect {
    stroke: var(--accent);
  }
  .node.selected > rect {
    stroke: var(--accent);
    stroke-width: calc(3px * var(--hair, 1));
  }
  .node.faded {
    opacity: 0.35;
  }
  .node.drop > rect {
    stroke: var(--ok);
    stroke-width: calc(3px * var(--hair, 1));
  }
  /* The stroke does not scale with the map, so a mark is never thinner than 3px. */
  .marks {
    cursor: pointer;
  }
  .mark {
    fill: var(--node-stroke);
    stroke: var(--node-stroke);
    stroke-width: 3px;
    stroke-linejoin: round;
    vector-effect: non-scaling-stroke;
  }
  .mark.menu {
    fill: var(--menu);
    stroke: var(--menu);
  }
  .mark.screen {
    fill: var(--screen);
    stroke: var(--screen);
  }
  .mark.missing {
    fill: var(--error);
    stroke: var(--error);
  }
  .mark.unreachable {
    fill: var(--dim);
    stroke: var(--dim);
    opacity: 0.7;
  }
  .mark.root {
    fill: var(--ok);
    stroke: var(--ok);
    stroke-width: 5px;
  }
  .marks.dim .mark:not(.near):not(.selected) {
    opacity: 0.3;
  }
  .mark.near {
    fill: var(--accent);
    stroke: var(--accent);
    stroke-width: 4px;
  }
  .mark.selected {
    fill: var(--accent);
    stroke: var(--accent);
    stroke-width: 7px;
  }
  .badge {
    pointer-events: none;
  }
  .badge text {
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.04em;
  }
  .badge.start rect {
    fill: var(--ok);
    stroke: none;
  }
  .badge.start text {
    fill: var(--on-accent);
  }
  .badge.end rect {
    fill: var(--panel);
    stroke: var(--dim);
    stroke-width: 1;
  }
  .badge.end text {
    fill: var(--dim);
  }
  .ret {
    fill: var(--dim);
    font-size: 12px;
    font-weight: 700;
    pointer-events: none;
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
