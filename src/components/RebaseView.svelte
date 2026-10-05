<script lang="ts">
  import { MergeView } from '@codemirror/merge'
  import { EditorState } from '@codemirror/state'
  import { EditorView, lineNumbers } from '@codemirror/view'
  import { onMount, untrack } from 'svelte'
  import { api, errorText } from '../lib/api'
  import { saveFile } from '../lib/edit.svelte'
  import { diffTheme } from '../lib/editor/theme'
  import { appearance } from '../lib/project.svelte'
  import type { RebaseResult } from '../lib/types'
  import Icon from './Icon.svelte'

  interface Props {
    path: string
  }

  let { path }: Props = $props()

  let host: HTMLDivElement
  let merge: MergeView | null = null
  let result = $state<RebaseResult | null>(null)
  let errorMsg = $state('')
  let loading = $state(true)
  let saving = $state(false)
  let at = 0

  function readOnly() {
    return [
      lineNumbers(),
      EditorState.readOnly.of(true),
      EditorView.editable.of(false),
      diffTheme(!appearance.light),
    ]
  }

  function draw() {
    merge?.destroy()
    merge = null
    if (!host || !result) return
    const right = result.baseMissing ? result.mine : result.merged
    merge = new MergeView({
      parent: host,
      a: { doc: result.upstream, extensions: readOnly() },
      b: { doc: right, extensions: [lineNumbers(), diffTheme(!appearance.light)] },
      highlightChanges: true,
      gutter: true,
    })
  }

  async function load() {
    loading = true
    errorMsg = ''
    try {
      result = await api.patchRebase(path)
      draw()
    } catch (e) {
      errorMsg = errorText(e)
    } finally {
      loading = false
    }
  }

  onMount(() => {
    void load()
    return () => merge?.destroy()
  })

  let themeSeen = appearance.light
  $effect(() => {
    const light = appearance.light
    if (light === themeSeen) return
    themeSeen = light
    untrack(() => draw())
  })

  function marks(): number[] {
    const text = merge?.b.state.doc.toString() ?? ''
    const found: number[] = []
    let i = 0
    while ((i = text.indexOf('<<<<<<<', i)) !== -1) {
      found.push(i)
      i += 7
    }
    return found
  }

  function jump(dir: number) {
    const view = merge?.b
    if (!view) return
    const found = marks()
    if (!found.length) return
    at = (at + dir + found.length) % found.length
    view.dispatch({ selection: { anchor: found[at] }, scrollIntoView: true })
    view.focus()
  }

  async function accept() {
    const view = merge?.b
    if (!view || saving) return
    if (marks().length && !confirm('The result still has conflict markers (<<<<<<<). Save it anyway?')) return
    saving = true
    try {
      await saveFile(path, view.state.doc.toString())
    } finally {
      saving = false
    }
  }
</script>

<div class="diff">
  <div class="head pane-head">
    <Icon name="diff" size={14} />
    <span class="dim path" title={path}>{path}</span>
    <span class="dim cap">
      {#if result?.baseMissing}
        upstream against your patch — no base snapshot
      {:else}
        upstream against the merged result{result ? ` · ${result.conflicts} conflict${result.conflicts === 1 ? '' : 's'}` : ''}
      {/if}
    </span>
    <span class="tools">
      {#if loading}<span class="dim">loading…</span>{/if}
      {#if result && !result.baseMissing}
        <button class="mini" onclick={() => jump(-1)}>Previous conflict</button>
        <button class="mini" onclick={() => jump(1)}>Next conflict</button>
      {/if}
      <button class="mini" onclick={accept} disabled={saving || loading || !result}>Accept</button>
    </span>
  </div>
  {#if errorMsg}<div class="err">{errorMsg}</div>{/if}
  <div class="host" bind:this={host}></div>
</div>

<style>
  .diff {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    background: var(--bg-code);
  }
  .head {
    min-width: 0;
    background: var(--bg-code);
    border-bottom-color: var(--line-soft);
  }
  .path,
  .cap,
  .dim {
    color: var(--dim);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tools {
    margin-left: auto;
    display: flex;
    gap: 6px;
    align-items: center;
    flex: none;
  }
  .host {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
  .host :global(.cm-editor),
  .host :global(.cm-mergeView) {
    height: 100%;
  }
  .err {
    color: var(--error);
    padding: 8px 12px;
    font-size: var(--fs-md);
  }
</style>
