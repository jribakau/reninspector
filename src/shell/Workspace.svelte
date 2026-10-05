<script lang="ts">
  import ActivityBar from '../components/ActivityBar.svelte'
  import BottomPanel from '../components/BottomPanel.svelte'
  import LiveStrip from '../components/LiveStrip.svelte'
  import Sidebar from '../components/Sidebar.svelte'
  import Splitter from '../components/Splitter.svelte'
  import CenterPane from './CenterPane.svelte'
  import EditorTabs from './EditorTabs.svelte'
  import { bindStage } from '../lib/stage.svelte'
  import { storedSize } from '../lib/pane'
  import { layout, settings } from '../lib/settings.svelte'
  import { bindPython, syncPython } from '../lib/pylsp.svelte'
  import { app } from '../lib/store.svelte'

  bindStage()
  void bindPython()
  $effect(() => {
    void settings.pythonServer
    void app.info
    syncPython()
  })

  const W_SIDE = 'vnide.w.side'
  const W_FLOW = 'vnide.w.flow'
  const H_BOTTOM = 'vnide.h.bottom'
  const H_STAGE = 'vnide.h.stage'
  let sideW = $state(storedSize(W_SIDE, 280))
  let flowW = $state(storedSize(W_FLOW, 420))
  let bottomH = $state(storedSize(H_BOTTOM, 200))
  let stageH = $state(storedSize(H_STAGE, 240))
  let layoutSeen = layout.seq
  $effect(() => {
    const seq = layout.seq
    if (seq === layoutSeen) return
    layoutSeen = seq
    sideW = storedSize(W_SIDE, 280)
    flowW = storedSize(W_FLOW, 420)
    bottomH = storedSize(H_BOTTOM, 200)
    stageH = storedSize(H_STAGE, 240)
  })

  function setSide(n: number) {
    sideW = n
    localStorage.setItem(W_SIDE, String(n))
  }
  function setFlow(n: number) {
    flowW = n
    localStorage.setItem(W_FLOW, String(n))
  }
  function setBottom(n: number) {
    bottomH = n
    localStorage.setItem(H_BOTTOM, String(n))
  }
  function setStage(n: number) {
    stageH = n
    localStorage.setItem(H_STAGE, String(n))
  }
</script>

<div class="workspace">
  <div class="body">
    <ActivityBar />
    <aside class:closed={!app.sidebarOpen} style={app.sidebarOpen ? `width:${sideW}px` : ''}>
      <Sidebar />
    </aside>
    {#if app.sidebarOpen}
      <Splitter axis="x" grow={1} value={sideW} min={200} hardMax={600} fraction={0.35} reserve={280} reset={280} onchange={setSide} />
    {/if}

    <section class="editor">
      <EditorTabs />
      {#if app.live.running}
        <LiveStrip />
      {/if}
      <CenterPane flowW={flowW} stageH={stageH} onflow={setFlow} onstage={setStage} />
    </section>
  </div>
  {#if app.bottomOpen}
    <Splitter axis="y" grow={-1} value={bottomH} min={120} hardMax={1600} fraction={0.7} reserve={180} reset={200} onchange={setBottom} />
    <section class="bottom" class:max={app.bottomMax} style={app.bottomMax ? '' : `height:${bottomH}px`}>
      <BottomPanel />
    </section>
  {/if}
</div>

<style>
  .workspace {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .body {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  aside {
    flex: none;
    min-width: 0;
    border-right: 1px solid var(--line);
    display: flex;
    flex-direction: column;
  }
  aside.closed {
    display: none;
  }
  .editor {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .bottom {
    flex: none;
    min-height: 0;
    overflow: hidden;
    border-top: 1px solid var(--line);
  }
  .bottom.max {
    flex: 3 1 0;
    height: auto;
  }
</style>
