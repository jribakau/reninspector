<script lang="ts">
  import Modal from '../components/Modal.svelte'
  import { closeDialog, dialog, type AskField } from '../lib/dialog.svelte'

  function optionsFor(field: AskField, value: string) {
    const options = field.options ?? []
    if (value && !options.some((opt) => opt.value === value)) return [...options, { value, label: value }]
    return options
  }

  let form: HTMLFormElement | undefined = $state()

  const ready = $derived.by(() => {
    const cur = dialog.current
    return !!cur && cur.fields.every((f, i) => f.optional || cur.values[i].trim() !== '')
  })

  function submit(e: SubmitEvent) {
    e.preventDefault()
    if (ready) closeDialog(true)
  }

  async function browse(i: number) {
    const cur = dialog.current
    const pick = cur?.fields[i].browse
    if (!cur || !pick) return
    const got = await pick()
    if (got && dialog.current === cur) cur.values[i] = got
  }
</script>

{#if dialog.current}
  {@const cur = dialog.current}
  <Modal title={cur.title} onclose={() => closeDialog(false)}>
    <form id="ask-form" bind:this={form} onsubmit={submit}>
      {#if cur.note}<p class="dim">{cur.note}</p>{/if}
      {#each cur.fields as field, i (i)}
        <label>
          {#if field.label}<span>{field.label}</span>{/if}
          <span class="field">
            {#if field.options}
              <select value={cur.values[i]} onchange={(e) => (cur.values[i] = e.currentTarget.value)}>
                {#each optionsFor(field, cur.values[i]) as opt (opt.value)}
                  <option value={opt.value}>{opt.label}</option>
                {/each}
              </select>
            {:else}
              <input
                type="text"
                spellcheck="false"
                value={cur.values[i]}
                placeholder={field.placeholder ?? ''}
                oninput={(e) => (cur.values[i] = e.currentTarget.value)}
              />
            {/if}
            {#if field.browse}
              <button type="button" onclick={() => browse(i)}>Browse…</button>
            {/if}
          </span>
        </label>
      {/each}
    </form>
    {#snippet footer()}
      <button type="button" onclick={() => closeDialog(false)}>Cancel</button>
      {#if cur.alt}
        <button type="button" onclick={() => closeDialog('alt')}>{cur.alt}</button>
      {/if}
      <button type="submit" form="ask-form" class="primary" disabled={!ready}>{cur.ok ?? 'OK'}</button>
    {/snippet}
  </Modal>
{/if}

<style>
  form {
    display: grid;
    gap: 10px;
  }
  form p {
    margin: 0;
    font-size: var(--fs-md);
  }
  label {
    display: grid;
    gap: 4px;
    font-size: var(--fs-md);
    color: var(--dim);
  }
  .field {
    display: flex;
    gap: 6px;
  }
  .field button {
    flex: none;
  }
  .field select {
    flex: 1;
    min-width: 0;
  }
</style>
