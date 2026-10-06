<script lang="ts">
  import { applog } from '../lib/applog.svelte'
  import { buildUi } from '../lib/build.svelte'
  import { app, openBottom, toggleBottom, type BottomTab } from '../lib/store.svelte'
  import BuildPanel from './BuildPanel.svelte'
  import ConsolePanel from './ConsolePanel.svelte'
  import EventsPanel from './EventsPanel.svelte'
  import Icon from './Icon.svelte'
  import LivePanel from './LivePanel.svelte'
  import ProblemsPanel from './ProblemsPanel.svelte'
  import TabStrip from './TabStrip.svelte'

  const problemCount = $derived((app.diag?.errors ?? 0) + (app.diag?.warnings ?? 0))

  const tabs: { id: BottomTab; label: string; count?: number; bad?: boolean; pulse?: boolean }[] = $derived([
    { id: 'problems', label: 'Problems', count: problemCount > 0 ? problemCount : undefined, bad: (app.diag?.errors ?? 0) > 0 },
    { id: 'log', label: 'Game', title: "The game's log.txt, traceback.txt and errors.txt" },
    {
      id: 'events',
      label: 'App',
      title: 'What Ren\'Inspector did: opens, saves, builds and errors',
      count: applog.unseenErrors > 0 ? applog.unseenErrors : undefined,
      bad: applog.unseenErrors > 0,
    },
    { id: 'build', label: 'Build', pulse: buildUi.running },
    { id: 'live', label: 'Live', pulse: app.live.running },
  ])
</script>

<div class="bottom">
  <TabStrip {tabs} active={app.bottomTab} prefix="bottom" label="Bottom panel" onselect={openBottom}>
    {#snippet tools()}
      <button
        class="icon"
        onclick={() => (app.bottomMax = !app.bottomMax)}
        title={app.bottomMax ? 'Restore panel' : 'Maximize panel'}
        aria-label={app.bottomMax ? 'Restore panel' : 'Maximize panel'}
      >
        <Icon name={app.bottomMax ? 'chevron-down' : 'chevron-up'} />
      </button>
      <button class="icon" onclick={toggleBottom} title="Close panel" aria-label="Close panel">
        <Icon name="close" />
      </button>
    {/snippet}
  </TabStrip>
  <div class="body">
    {#each tabs as t (t.id)}
      <div
        class="fill"
        class:hidden={app.bottomTab !== t.id}
        role="tabpanel"
        id={`bottom-panel-${t.id}`}
        aria-labelledby={`bottom-tab-${t.id}`}
      >
        {#if t.id === 'problems'}<ProblemsPanel />
        {:else if t.id === 'log'}<ConsolePanel />
        {:else if t.id === 'events'}<EventsPanel />
        {:else if t.id === 'build'}<BuildPanel />
        {:else}<LivePanel />{/if}
      </div>
    {/each}
  </div>
</div>

<style>
  .bottom {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    background: var(--panel);
  }
  .body {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .fill {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }
  .fill.hidden {
    display: none;
  }
</style>
