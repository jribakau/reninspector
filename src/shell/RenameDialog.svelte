<script lang="ts">
  import Icon from '../components/Icon.svelte'
  import Modal from '../components/Modal.svelte'
  import { app, applyRename, cancelRename } from '../lib/store.svelte'

  function requestClose() {
    if (app.busy) return
    cancelRename()
  }
</script>

{#if app.renamePreview}
  {@const preview = app.renamePreview}
  {@const sample = preview.hits.slice(0, 12)}
  <Modal title={`Rename ${preview.oldName} to ${preview.newName}`} wide busy={!!app.busy} onclose={requestClose}>
      {#if preview.truncated}
        <p>More than 300 lines match. Rename was not applied.</p>
      {:else}
        <p>
          {preview.fileCount} file{preview.fileCount === 1 ? '' : 's'},
          {preview.hits.length} line{preview.hits.length === 1 ? '' : 's'}.
        </p>
      {/if}
      <ul>
        {#each sample as hit (`${hit.path}:${hit.line}:${hit.before}`)}
          <li>
            <code>{hit.path}:{hit.line}</code>
            <span>{hit.before}</span>
            <span class="arrow"><Icon name="arrow-right" size={12} /></span>
            <span>{hit.after}</span>
          </li>
        {/each}
      </ul>
      {#if preview.hits.length > sample.length}
        <p class="dim">and {(preview.hits.length - sample.length).toLocaleString()} more</p>
      {/if}
      {#if preview.notes.length}
        <p>These style prefixes still build this name, and were left unchanged.</p>
        <ul>
          {#each preview.notes.slice(0, 8) as note (`${note.path}:${note.line}:${note.text}`)}
            <li>
              <code>{note.path}:{note.line}</code>
              <span>{note.text}</span>
            </li>
          {/each}
        </ul>
      {/if}
      {#if app.error}<p class="bad">{app.error}</p>{/if}
    {#snippet footer()}
      <button onclick={cancelRename} disabled={!!app.busy}>Cancel</button>
      {#if !preview.truncated && preview.hits.length}
        <button class="primary apply" onclick={applyRename} disabled={!!app.busy}>
          {#if app.busy}<i class="spinner"></i>{/if}
          {app.busy ? 'Applying…' : 'Apply'}
        </button>
      {/if}
    {/snippet}
  </Modal>
{/if}

<style>
  p {
    margin: 0;
    font-size: var(--fs-md);
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: 6px;
  }
  li {
    font-size: var(--fs-md);
    display: grid;
    gap: 2px;
  }
  code {
    color: var(--dim);
    font-size: var(--fs-sm);
  }
  .arrow {
    color: var(--dim);
    display: inline-flex;
    vertical-align: middle;
  }
  .bad {
    color: var(--error);
  }
  .apply {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-2);
  }
  .apply .spinner {
    width: 12px;
    height: 12px;
    border-width: 2px;
  }
</style>
