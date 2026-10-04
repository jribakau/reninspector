<script lang="ts">
  import Modal from '../components/Modal.svelte'
  import { browseBuildDest, buildMismatch, buildTarget, buildUi, closeBuildDialog, startBuild } from '../lib/build.svelte'
  import { app } from '../lib/model.svelte'
  import { projectVersion } from '../lib/sdk.svelte'

  const target = $derived(buildTarget())
  const mismatch = $derived(buildMismatch())
  const canBuild = $derived(!!target && !buildUi.running && (buildUi.pc || buildUi.mac || buildUi.web))

  $effect(() => {
    void buildUi.pc
    void buildUi.mac
    void buildUi.web
    buildUi.message = ''
  })
</script>

{#if buildUi.dialog}
  <Modal title="Build" onclose={closeBuildDialog}>
      {#if target}
        <p class="dim">Using Ren'Py {target.version ?? target.path}.</p>
      {:else}
        <p class="warn">Install a Ren'Py SDK before building. The game's own engine cannot package a release.</p>
      {/if}
      {#if mismatch}
        <p class="warn">
          This SDK is {target?.version}, and the project is {app.info ? projectVersion(app.info) : ''}. A newer SDK
          recompiles scripts and can change save compatibility.
        </p>
      {/if}
      <label class="check"><input type="checkbox" bind:checked={buildUi.pc} /> Windows and Linux</label>
      <label class="check"><input type="checkbox" bind:checked={buildUi.mac} /> Mac</label>
      <label class="check"><input type="checkbox" bind:checked={buildUi.web} /> Web</label>
      {#if buildUi.web}
        <label class="check"><input type="checkbox" bind:checked={buildUi.launch} /> Open in browser when the build finishes</label>
        {#if target && !target.hasWeb}
          <p class="dim">Web support is not in this SDK yet. It will be downloaded first.</p>
        {/if}
      {/if}
      <label>
        <span>Destination folder</span>
        <span class="field">
          <input
            type="text"
            spellcheck="false"
            placeholder="Next to the project, in a -dists folder"
            bind:value={buildUi.dest}
          />
          <button type="button" onclick={() => void browseBuildDest()}>Browse…</button>
        </span>
      </label>
      {#if buildUi.message}<p class="bad">{buildUi.message}</p>{/if}
    {#snippet footer()}
      <button type="button" onclick={closeBuildDialog}>Cancel</button>
      <button type="button" class="primary" disabled={!canBuild} onclick={() => void startBuild()}>Build</button>
    {/snippet}
  </Modal>
{/if}

<style>
  p { margin: 0; }
  .dim { color: var(--dim); font-size: var(--fs-md); }
  .warn { color: var(--warning); font-size: var(--fs-md); }
  .bad { color: var(--error); font-size: var(--fs-md); }
  label { display: grid; gap: 4px; font-size: var(--fs-md); color: var(--dim); }
  .check { display: flex; gap: 8px; align-items: center; color: var(--text); }
  .field { display: flex; gap: 6px; }
  .field button { flex: none; }
</style>
