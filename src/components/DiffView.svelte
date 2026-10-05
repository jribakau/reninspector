<script lang="ts">
  import { getChunks, getOriginalDoc, goToNextChunk, goToPreviousChunk, MergeView, unifiedMergeView } from '@codemirror/merge'
  import { EditorState } from '@codemirror/state'
  import { EditorView, lineNumbers } from '@codemirror/view'
  import { onMount, untrack } from 'svelte'
  import { api, errorText } from '../lib/api'
  import { takeChunk } from '../lib/editor/chunks'
  import { diffTheme } from '../lib/editor/theme'
  import { git, loadGit } from '../lib/git.svelte'
  import { appearance } from '../lib/project.svelte'
  import { setSetting, settings } from '../lib/settings.svelte'
  import Icon from './Icon.svelte'
  import { goTo } from '../lib/store.svelte'

  interface Props {
    path: string
    /** `INDEX` is the working copy against the index. `STAGED` is the index against HEAD. `HEAD` is the working copy against HEAD. A rev ending in `^` compares that commit with its parent. */
    rev: string
  }

  let { path, rev }: Props = $props()

  let host: HTMLDivElement
  let editor: EditorView | null = null
  let merge: MergeView | null = null
  const split = $derived(settings.diffSplit)
  let errorMsg = $state('')
  let loading = $state(false)
  let staging = $state(false)
  let identical = $state(false)
  let shownKey = ''
  let rawOriginal = ''
  let ticket = 0

  function sides(left: string): { original: string; current: string } {
    if (left === 'INDEX') return { original: 'INDEX', current: 'WORKTREE' }
    if (left === 'STAGED') return { original: 'HEAD', current: 'INDEX' }
    if (left.endsWith('^')) return { original: left, current: left.slice(0, -1) }
    return { original: left, current: 'WORKTREE' }
  }

  function caption(left: string): string {
    if (left === 'INDEX') return 'working copy against staged'
    if (left === 'STAGED') return 'staged against HEAD'
    if (left.endsWith('^')) return `${left.slice(0, -1).slice(0, 7)} against its parent`
    return `working copy against ${left}`
  }

  function baseExtensions() {
    return [
      lineNumbers(),
      EditorState.readOnly.of(true),
      EditorView.editable.of(false),
      diffTheme(!appearance.light),
    ]
  }

  function destroyViews() {
    editor?.destroy()
    merge?.destroy()
    editor = null
    merge = null
  }

  function active(): EditorView | null {
    return merge?.b ?? editor
  }

  async function load(force = false) {
    const mine = ++ticket
    if (!editor && !merge) loading = true
    errorMsg = ''
    const pair = sides(rev)
    try {
      const [original, current] = await Promise.all([api.gitShow(pair.original, path), api.gitShow(pair.current, path)])
      if (mine !== ticket || !host) return
      rawOriginal = original
      identical = original === current
      const key = `${split ? 's' : 'i'}\0${original}\0${current}`
      if (!force && key === shownKey && (editor || merge)) return
      shownKey = key
      const scroll = active()?.scrollDOM.scrollTop ?? 0
      destroyViews()
      if (split) {
        merge = new MergeView({
          parent: host,
          a: { doc: original, extensions: baseExtensions() },
          b: { doc: current, extensions: baseExtensions() },
          highlightChanges: true,
          gutter: true,
        })
      } else {
        editor = new EditorView({
          parent: host,
          state: EditorState.create({
            doc: current,
            extensions: [...baseExtensions(), unifiedMergeView({ original, mergeControls: false })],
          }),
        })
      }
      const view = active()
      if (view && scroll) requestAnimationFrame(() => (view.scrollDOM.scrollTop = scroll))
    } catch (e) {
      if (mine === ticket) errorMsg = errorText(e)
    } finally {
      if (mine === ticket) loading = false
    }
  }

  // Staging, committing, or saving elsewhere changes what this tab compares.
  $effect(() => {
    void git.seq
    untrack(() => void load())
  })

  // Switching theme rebuilds the editors so the merge colors follow.
  let themeSeen = appearance.light
  $effect(() => {
    const light = appearance.light
    if (light === themeSeen) return
    themeSeen = light
    untrack(() => void load(true))
  })

  // Inline or side by side, from the toolbar or from Settings.
  let splitSeen = settings.diffSplit
  $effect(() => {
    const now = settings.diffSplit
    if (now === splitSeen) return
    splitSeen = now
    untrack(() => void load(true))
  })

  function jump(next: boolean) {
    const view = active()
    if (!view) return
    ;(next ? goToNextChunk : goToPreviousChunk)(view)
  }

  function toggleSplit() {
    setSetting('diffSplit', !split)
  }

  async function stageChange() {
    const view = active()
    if (!view || rev !== 'INDEX' || staging) return
    const info = getChunks(view.state)
    const pos = view.state.selection.main.head
    const line = view.state.doc.lineAt(pos).number
    const chunk =
      info?.chunks.find((c) => pos >= c.fromB && pos <= Math.max(c.endB, c.fromB)) ??
      info?.chunks.find((c) => c.fromB === c.toB && view.state.doc.lineAt(Math.min(c.fromB, view.state.doc.length)).number === line)
    if (!chunk) {
      errorMsg = 'Put the cursor on a change first.'
      return
    }
    const original = merge ? merge.a.state.doc : getOriginalDoc(view.state)
    let next = takeChunk(original, view.state.doc, chunk)
    // The editors hold LF text. Keep the line endings the index already has.
    if (rawOriginal.includes('\r\n')) next = next.replace(/\n/g, '\r\n')
    staging = true
    errorMsg = ''
    try {
      await api.gitStageText(path, next)
      await loadGit()
    } catch (e) {
      errorMsg = errorText(e)
    } finally {
      staging = false
    }
  }

  function openFile() {
    const rel = path.replaceAll('\\', '/')
    if (rel.startsWith('game/') && (rel.endsWith('.rpy') || rel.endsWith('.rpym'))) {
      goTo(rel.slice(5), 1, 1, { flow: true })
    }
  }

  onMount(() => () => destroyViews())
</script>

<div class="diff">
  <div class="pane-head code">
    <Icon name="diff" size={14} />
    <span class="dim path" title={path}>{path}</span>
    <span class="dim cap">{caption(rev)}</span>
    <span class="pane-tools">
      {#if loading}<span class="dim">loading…</span>{/if}
      <button class="mini icon" onclick={() => jump(false)} title="Previous change" aria-label="Previous change"><Icon name="arrow-up" size={13} /></button>
      <button class="mini icon" onclick={() => jump(true)} title="Next change" aria-label="Next change"><Icon name="arrow-down" size={13} /></button>
      <button class="mini" onclick={toggleSplit}>{split ? 'Inline' : 'Side by side'}</button>
      {#if rev === 'INDEX'}
        <button class="mini" onclick={stageChange} disabled={staging || loading}>Stage change</button>
      {/if}
      <button class="mini" onclick={openFile}>Open file</button>
    </span>
  </div>
  {#if errorMsg}<div class="banner-err">{errorMsg}</div>{/if}
  <div class="host" bind:this={host}></div>
  {#if identical && !loading && !errorMsg}
    <div class="same empty-state">
      <Icon name="check" size={20} />
      <h2>No changes</h2>
      <p>Comparing {caption(rev)}: both sides match.</p>
    </div>
  {/if}
</div>

<style>
  .diff {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    background: var(--bg-code);
  }
  .diff {
    position: relative;
  }
  .same {
    position: absolute;
    inset: 30px 0 0;
    background: var(--bg-code);
    pointer-events: none;
  }
  .path {
    flex: 0 1 auto;
  }
  .cap {
    flex: 0 100 auto;
  }
  .dim {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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
</style>
