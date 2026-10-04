<script lang="ts">
  import { app } from '../lib/store.svelte'
  import ExplorerPanel from './ExplorerPanel.svelte'
  import GitPanel from './GitPanel.svelte'
  import RenpyPanel from './RenpyPanel.svelte'
  import SearchPanel from './SearchPanel.svelte'
  import StoryPanel from './StoryPanel.svelte'
</script>

<div class="side">
  <div class="pane" class:hidden={app.activity !== 'explorer'}><ExplorerPanel /></div>
  <div class="pane" class:hidden={app.activity !== 'story'}><StoryPanel /></div>
  <div class="pane" class:hidden={app.activity !== 'search'}><SearchPanel /></div>
  <div class="pane" class:hidden={app.activity !== 'renpy'}><RenpyPanel /></div>
  <div class="pane" class:hidden={app.activity !== 'git'}><GitPanel /></div>
</div>

<style>
  .side {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    background: var(--panel);
  }
  .pane {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }
  .pane.hidden {
    display: none;
  }
</style>
