<script lang="ts">
  import Breadcrumbs from '../components/Breadcrumbs.svelte'
  import CodeView from '../components/CodeView.svelte'
  import DiffView from '../components/DiffView.svelte'
  import FilePreview from '../components/FilePreview.svelte'
  import LabelGraph from '../components/LabelGraph.svelte'
  import ProjectMap from '../components/ProjectMap.svelte'
  import RebaseView from '../components/RebaseView.svelte'
  import Splitter from '../components/Splitter.svelte'
  import StagePane from '../components/StagePane.svelte'
  import { previewKind } from '../lib/preview'
  import { fileEditable, originHint } from '../lib/tabs'
  import {
    app,
    cursorMoved,
    diagnosticsFor,
    editorTabId,
    fileInfo,
    fileOfNode,
    followFlowLabel,
    enclosingAt,
    goTo,
    nodeByName,
    nodesInFile,
    noteStale,
    openActivity,
    openLabelGraph,
    openMapTab,
    renameSymbol,
    showReferences,
    revertFile,
    saveFile,
    revealInExplorer,
    selectLabel,
    setDirty,
  } from '../lib/store.svelte'

  interface Props {
    flowW: number
    stageH: number
    onflow: (n: number) => void
    onstage: (n: number) => void
  }

  let { flowW, stageH, onflow, onstage }: Props = $props()

  const activeTab = $derived(app.editorTabs.find((t) => editorTabId(t) === app.activeEditor) ?? null)
  const activeKind = $derived(activeTab?.kind ?? 'file')
  const openedPreview = $derived(app.loc && !fileInfo(app.loc.file) ? previewKind(app.loc.file) : null)
  const mediaKind = $derived(openedPreview === 'image' || openedPreview === 'audio' || openedPreview === 'video' ? openedPreview : null)
  const sceneMain = $derived(app.sceneExpanded && activeKind === 'file' && app.editorTabs.length > 0)
  const fileCenter = $derived(activeTab?.kind === 'file')

  $effect(() => {
    if (!sceneMain) return
    const el = document.activeElement
    if (el instanceof HTMLElement && el.closest('.pane.hidden')) el.blur()
  })

  const graphName = $derived(activeTab?.kind === 'graph' ? activeTab.name : null)
  const diffTab = $derived(activeTab?.kind === 'diff' ? activeTab : null)
  const rebaseTab = $derived(activeTab?.kind === 'rebase' ? activeTab : null)
  const graphNode = $derived(graphName ? nodeByName(graphName) : undefined)
  const graphFile = $derived(graphNode ? fileOfNode(graphNode) : null)

  const flowNode = $derived.by(() => {
    if (!app.selectedLabel) return undefined
    const node = nodeByName(app.selectedLabel)
    if (!node || node.kind === 'missing' || node.kind === 'screen' || node.kind === 'compiled') return undefined
    return node
  })
  const flowFile = $derived(flowNode ? fileOfNode(flowNode) : null)
  const flowEmpty = $derived.by(() => {
    const file = app.loc?.file
    if (!file) return 'Open a script to see its flow.'
    const has = nodesInFile(file).some((n) => n.kind === 'label' || n.kind === 'menu')
    return has ? 'Move the caret into a label to see its flow.' : 'This script has no label to draw.'
  })

  const crumbs = $derived.by(() => {
    if (!app.loc || !app.cursor || app.cursor.file !== app.loc.file) return []
    return enclosingAt(app.loc.file, app.cursor.line)
  })

  function openCrumb(id: string) {
    const crumb = crumbs.find((c) => c.id === id)
    if (!crumb || !app.loc) return
    if (crumb.kind === 'transform' || crumb.kind === 'style') {
      goTo(app.loc.file, crumb.line, crumb.endLine)
      app.selectedLabel = crumb.id
      return
    }
    selectLabel(crumb.id)
  }

  const openDiags = $derived.by(() => {
    void app.diag
    return diagnosticsFor(app.loc?.file)
  })

  function cursorLine(file: string | null): number | null {
    return app.cursor && file && app.cursor.file === file ? app.cursor.line : null
  }

  function liveLine(file: string | null): number | null {
    return app.live.running && file && app.live.file === file ? app.live.line : null
  }

  function editing(path: string | undefined): boolean {
    if (!path || !app.info) return true
    return fileEditable(fileInfo(path))
  }

  function hint(path: string | undefined): string {
    if (!path || !app.info) return ''
    return originHint(fileInfo(path))
  }
</script>

<div class="editor-row">
  <div class="stage">
    {#if !app.editorTabs.length}
      <div class="pane empty-workspace empty-state">
        <h2>{app.info?.name ?? 'No file open'}</h2>
        <p>Open a script, or jump somewhere in the project.</p>
        <div class="empty-actions">
          <button class="primary" onclick={() => (app.palette = 'files')}>Go to file</button>
          <button onclick={openMapTab}>Project map</button>
          {#if app.recentFiles.length}
            <button onclick={() => goTo(app.recentFiles[0], 1)}>Open recent</button>
          {:else}
            <button onclick={() => openActivity('explorer')}>Show explorer</button>
          {/if}
        </div>
        <ul class="shortcuts" aria-label="Keyboard shortcuts">
          <li><span class="kbd">Ctrl</span><span class="kbd">P</span> Go to file</li>
          <li><span class="kbd">Ctrl</span><span class="kbd">Shift</span><span class="kbd">P</span> Commands</li>
          <li><span class="kbd">Ctrl</span><span class="kbd">T</span> Go to symbol</li>
        </ul>
      </div>
    {/if}
    <div class="pane" class:hidden={activeKind !== 'file' || sceneMain || !!mediaKind || !app.editorTabs.length}>
      {#if app.loc}
        <Breadcrumbs path={app.loc.file} symbols={crumbs} onsegment={revealInExplorer} onsymbol={openCrumb} />
      {/if}
      <CodeView
        loc={mediaKind ? null : app.loc}
        diagnostics={openDiags}
        changeSeq={app.changeSeq}
        changedPaths={app.changedPaths}
        editing={editing(app.loc?.file)}
        dirty={!!app.loc && app.dirtyFiles.includes(app.loc.file)}
        modified={!!app.loc && app.modifiedFiles.includes(app.loc.file)}
        reloadFile={app.reloadFile}
        liveLine={liveLine(app.loc?.file ?? null)}
        oncursor={cursorMoved}
        ondirty={setDirty}
        onsave={saveFile}
        onrevert={revertFile}
        onstale={noteStale}
        ongoto={goTo}
        onrefs={showReferences}
        onrename={renameSymbol}
        hint={hint(app.loc?.file)}
      />
    </div>
    {#if mediaKind && app.loc && activeKind === 'file' && !sceneMain}
      <div class="pane">
        <Breadcrumbs path={app.loc.file} onsegment={revealInExplorer} />
        {#key app.loc.file}
          <FilePreview path={app.loc.file} kind={mediaKind} />
        {/key}
      </div>
    {/if}
    {#if graphName}
      <div class="pane">
        {#key graphName}
          <LabelGraph
            name={graphName}
            file={graphFile}
            cursorLine={cursorLine(graphFile)}
            liveLine={liveLine(graphFile)}
            reloadKey={app.changeSeq}
            onopen={followFlowLabel}
            ongoto={goTo}
          />
        {/key}
      </div>
    {/if}
    {#if sceneMain}
      <div class="pane">
        {#if flowNode}
          {#key flowNode.id}
            <LabelGraph
              name={flowNode.id}
              file={flowFile}
              cursorLine={cursorLine(flowFile)}
              liveLine={liveLine(flowFile)}
              reloadKey={app.changeSeq}
              onopen={followFlowLabel}
              ongoto={goTo}
            />
          {/key}
        {:else}
          <div class="flow-empty">{flowEmpty}</div>
        {/if}
      </div>
    {/if}
    {#if diffTab}
      <div class="pane">
        {#key `${diffTab.rev}:${diffTab.path}`}
          <DiffView path={diffTab.path} rev={diffTab.rev} />
        {/key}
      </div>
    {/if}
    {#if rebaseTab}
      <div class="pane">
        {#key rebaseTab.path}
          <RebaseView path={rebaseTab.path} />
        {/key}
      </div>
    {/if}
    {#if activeKind === 'map' && app.map}
      <div class="pane">
        <ProjectMap
          map={app.map}
          files={app.info?.files ?? []}
          root={app.info?.root ?? ''}
          selected={app.selectedLabel}
          onselect={(n) => selectLabel(n)}
          onopen={openLabelGraph}
        />
      </div>
    {/if}
  </div>
  {#if (app.stageOpen || app.flowOpen) && !sceneMain && fileCenter}
    <Splitter axis="x" grow={-1} value={flowW} min={280} hardMax={800} fraction={0.45} reserve={240} reset={420} onchange={onflow} />
    <section class="flow-pane" style={`width:${flowW}px`}>
      {#if app.stageOpen}
        <div class="stage-slot" style={app.flowOpen ? `flex: 0 0 ${stageH}px` : ''}>
          <StagePane />
        </div>
      {/if}
      {#if app.stageOpen && app.flowOpen}
        <Splitter axis="y" grow={1} value={stageH} min={80} hardMax={900} fraction={0.8} reserve={80} reset={240} onchange={onstage} />
      {/if}
      {#if app.flowOpen}
        <div class="flow-slot">
          {#if flowNode}
            {#key flowNode.id}
              <LabelGraph
                name={flowNode.id}
                file={flowFile}
                cursorLine={cursorLine(flowFile)}
                liveLine={liveLine(flowFile)}
                reloadKey={app.changeSeq}
                onopen={followFlowLabel}
                ongoto={goTo}
              />
            {/key}
          {:else}
            <div class="flow-empty">{flowEmpty}</div>
          {/if}
        </div>
      {/if}
    </section>
  {/if}
</div>

<style>
  .editor-row { flex: 1; min-height: 0; display: flex; }
  .stage {
    flex: 1;
    min-width: 0;
    position: relative;
    background: var(--bg-canvas);
  }
  .pane {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
  }
  .pane.hidden { visibility: hidden; pointer-events: none; }
  .empty-workspace { z-index: var(--z-base); }
  .empty-actions {
    display: flex;
    gap: var(--sp-3);
    flex-wrap: wrap;
    justify-content: center;
    margin-bottom: var(--sp-3);
  }
  .shortcuts {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: var(--sp-3);
    font-size: var(--fs-md);
  }
  .shortcuts li { display: flex; align-items: center; gap: var(--sp-1); }
  .shortcuts li :global(.kbd:last-of-type) { margin-right: var(--sp-3); }
  .flow-pane {
    flex: none;
    min-width: 0;
    min-height: 0;
    height: 100%;
    border-left: 1px solid var(--line);
    position: relative;
    background: var(--bg-canvas);
    display: flex;
    flex-direction: column;
  }
  .stage-slot {
    flex: 1 1 auto;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    border-bottom: 1px solid var(--line);
  }
  .flow-pane:not(:has(.flow-slot)) .stage-slot { flex: 1 1 auto; }
  .flow-slot { flex: 1; min-height: 0; position: relative; overflow: hidden; }
  .flow-slot :global(.lg) { height: 100%; }
  .flow-empty {
    height: 100%;
    display: grid;
    place-items: center;
    color: var(--dim);
    padding: var(--sp-5);
    text-align: center;
  }
</style>
