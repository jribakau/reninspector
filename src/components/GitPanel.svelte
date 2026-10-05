<script lang="ts">
  import EmptyState from './EmptyState.svelte'
  import Icon from './Icon.svelte'
  import { api, errorText } from '../lib/api'
  import { copyText, openContextMenu, type MenuEntry } from '../lib/context.svelte'
  import { askText } from '../lib/dialog.svelte'
  import { reloadEditor } from '../lib/edit.svelte'
  import { git, loadGit, openBranchPicker } from '../lib/git.svelte'
  import { filesUnder, treeOf, type GitFolder } from '../lib/git-tree'
  import { setSetting, settings } from '../lib/settings.svelte'
  import { app, goTo, openDiff, revealInExplorer } from '../lib/store.svelte'
  import { notify } from '../lib/toast.svelte'
  import type { GitChange, GitCommit, GitCommitFile } from '../lib/types'

  let commits = $state<GitCommit[]>([])
  let openHash = $state('')
  let commitFiles = $state<GitCommitFile[]>([])
  let timeline = $state<GitCommit[]>([])
  let actionErr = $state('')
  let message = $state('')
  let busy = $state(false)
  let stagedOpen = $state(true)
  let changesOpen = $state(true)
  let historyOpen = $state(true)
  let timelineOpen = $state(true)
  let menu = $state<null | 'commit' | 'more'>(null)
  let ignoreDismissed = $state(false)
  const tree = $derived(settings.gitTree)
  let closedFolders = $state<Set<string>>(new Set())
  let timelineEl = $state<HTMLDivElement | null>(null)
  let seenTimeline = 0
  let rootSeen = ''

  const status = $derived(git.status)
  const notRepo = $derived(git.err.includes('not a git repository'))
  const shownErr = $derived(actionErr || (notRepo ? '' : git.err))
  const staged = $derived((status?.changes ?? []).filter((c) => c.staged))
  const unstaged = $derived((status?.changes ?? []).filter((c) => !c.staged))
  const branch = $derived(status?.branch || 'HEAD')
  const timelineFile = $derived(git.historyFile || app.loc?.file || '')
  const timelineName = $derived(timelineFile.split('/').pop() ?? '')
  const ROW_STEP = 300
  let stagedShown = $state(ROW_STEP)
  let changesShown = $state(ROW_STEP)
  const stagedRows = $derived(staged.slice(0, stagedShown))
  const changeRows = $derived(unstaged.slice(0, changesShown))
  const panelOpen = $derived(app.sidebarOpen && app.activity === 'git')
  const outside = $derived(status?.outsideStaged ?? [])
  const commitBlocked = $derived(outside.length > 0)
  const commitBlock = 'Files outside this project are staged. Unstage them before committing here.'

  const stagedTree = $derived(treeOf(stagedRows))
  const changeTree = $derived(treeOf(changeRows))

  $effect(() => {
    const root = app.info?.root ?? ''
    if (root === rootSeen) return
    rootSeen = root
    ignoreDismissed = false
    stagedShown = ROW_STEP
    changesShown = ROW_STEP
  })

  $effect(() => {
    void git.seq
    if (!panelOpen || !git.status) {
      if (!git.status) commits = []
      return
    }
    let alive = true
    void api.gitLog(null, 30).then(
      (list) => {
        if (alive) commits = list
      },
      () => {
        if (alive) commits = []
      },
    )
    return () => {
      alive = false
    }
  })

  // Opening another file ends a history request made for a different one.
  $effect(() => {
    void app.loc?.file
    git.historyFile = ''
  })

  $effect(() => {
    const file = timelineFile
    void git.seq
    if (!panelOpen || !file || !git.status) {
      if (!file || !git.status) timeline = []
      return
    }
    let alive = true
    void api.gitLog(`game/${file}`, 50).then(
      (list) => {
        if (alive) timeline = list
      },
      () => {
        if (alive) timeline = []
      },
    )
    return () => {
      alive = false
    }
  })

  $effect(() => {
    const n = git.timeline
    if (!n || n === seenTimeline) return
    seenTimeline = n
    timelineOpen = true
    timelineEl?.scrollIntoView({ block: 'nearest' })
  })

  $effect(() => {
    if (!menu) return
    const close = (event: PointerEvent) => {
      const target = event.target
      if (target instanceof Element && (target.closest('.pop') || target.closest('.pop-wrap') || target.closest('.commit-split'))) return
      menu = null
    }
    window.addEventListener('pointerdown', close, true)
    return () => window.removeEventListener('pointerdown', close, true)
  })

  function gameScript(path: string): string | null {
    const rel = path.replaceAll('\\', '/')
    if (!rel.startsWith('game/')) return null
    const game = rel.slice(5)
    return game.endsWith('.rpy') || game.endsWith('.rpym') ? game : null
  }

  function fileParts(path: string): { name: string; dir: string } {
    const rel = path.replaceAll('\\', '/')
    const cut = rel.lastIndexOf('/')
    if (cut < 0) return { name: rel, dir: '' }
    return { name: rel.slice(cut + 1), dir: rel.slice(0, cut) }
  }

  function statusLetter(code: string): string {
    if (code === '?') return 'U'
    return code.slice(0, 1).toUpperCase()
  }

  function statusClass(code: string): string {
    const letter = code.slice(0, 1).toUpperCase()
    if (letter === 'D') return 'deleted'
    if (letter === 'A' || letter === '?') return 'added'
    if (letter === 'R' || letter === 'C') return 'renamed'
    return 'modified'
  }

  function diffRev(change: GitChange): string {
    return change.staged ? 'STAGED' : 'INDEX'
  }

  function openFile(path: string) {
    const rel = path.replaceAll('\\', '/')
    const script = gameScript(rel)
    if (script) goTo(script, 1, 1, { flow: true })
    else if (rel.startsWith('game/')) revealInExplorer(rel.slice(5))
  }

  function firstLine(text: string): string {
    return text.split('\n').map((line) => line.trim()).find(Boolean) ?? 'Done.'
  }

  function filesNote(verb: string, count: number): string {
    return `${verb} ${count} file${count === 1 ? '' : 's'}.`
  }

  async function run(work: () => Promise<string | void>) {
    busy = true
    actionErr = ''
    menu = null
    try {
      const note = await work()
      await loadGit()
      if (note) notify(note, 'ok')
    } catch (e) {
      actionErr = errorText(e)
    } finally {
      busy = false
    }
  }

  async function initRepo() {
    await run(async () => {
      await api.gitInit()
      return 'Repository initialized.'
    })
  }

  async function commit(amend = false) {
    const text = message.trim()
    if (busy || commitBlocked || (!text && !amend)) return
    if (amend && status?.headPushed) {
      if (!confirm('This commit is already on the remote. Amending it means you will have to force-push.')) return
    }
    await run(async () => {
      const result = await api.gitCommit(text, amend)
      message = ''
      return firstLine(result)
    })
  }

  async function commitAndPush() {
    const text = message.trim()
    if (busy || commitBlocked || !text) return
    await run(async () => {
      await api.gitCommit(text, false)
      message = ''
      await api.gitPush()
      return 'Committed and pushed.'
    })
  }

  async function undoCommit() {
    const note = status?.headPushed
      ? 'This commit is already on the remote. Undoing it only changes your copy.'
      : 'Undo the last commit? Its changes stay staged.'
    if (!confirm(note)) return
    await run(async () => {
      const subject = await api.gitUndoCommit()
      if (!message.trim()) message = subject
      return 'Undid the last commit.'
    })
  }

  async function sync() {
    const behind = status?.behind ?? 0
    const ahead = status?.ahead ?? 0
    if (behind > 0 && ahead > 0) {
      actionErr = 'Your branch and the remote have both changed. Pull with merge or rebase in a terminal.'
      return
    }
    await run(async () => {
      if (behind) await api.gitPull()
      if (ahead) await api.gitPush()
      return 'Synced.'
    })
  }

  async function addRemote() {
    const url = await askText('Add remote', 'https://', 'Add')
    if (!url) return
    await run(async () => {
      await api.gitAddRemote(url)
      return 'Added origin.'
    })
  }

  function onMessageKey(e: KeyboardEvent) {
    if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault()
      void commit(false)
    }
  }

  async function discard(path: string) {
    const { name } = fileParts(path)
    if (!confirm(`Discard changes to ${name}?`)) return
    await run(async () => {
      await api.gitDiscard(path)
      const script = gameScript(path)
      if (script) reloadEditor(script)
      return `Discarded changes to ${name}.`
    })
  }

  async function discardMany(changes: GitChange[]) {
    const unstagedOnly = changes.filter((c) => !c.staged)
    if (!unstagedOnly.length) return
    if (!confirm(`Discard changes to ${unstagedOnly.length} file${unstagedOnly.length === 1 ? '' : 's'}?`)) return
    await run(async () => {
      for (const change of unstagedOnly) {
        await api.gitDiscard(change.path)
        const script = gameScript(change.path)
        if (script) reloadEditor(script)
      }
      return filesNote('Discarded changes to', unstagedOnly.length)
    })
  }

  async function toggleCommit(hash: string) {
    if (openHash === hash) {
      openHash = ''
      commitFiles = []
      return
    }
    openHash = hash
    commitFiles = []
    try {
      commitFiles = await api.gitCommitFiles(hash)
      actionErr = ''
    } catch (e) {
      actionErr = errorText(e)
    }
  }

  function toggleTree() {
    setSetting('gitTree', !tree)
  }

  function toggleFolder(path: string) {
    const next = new Set(closedFolders)
    if (next.has(path)) next.delete(path)
    else next.add(path)
    closedFolders = next
  }

  function fileMenu(c: GitChange): MenuEntry[] {
    return [
      { kind: 'item', label: 'Open file', run: () => openFile(c.path) },
      { kind: 'item', label: 'Open changes', run: () => openDiff(c.path, diffRev(c)) },
      {
        kind: 'item',
        label: c.staged ? 'Unstage changes' : 'Stage changes',
        enabled: !busy,
        run: () => void run(async () => {
          if (c.staged) {
            await api.gitUnstage([c.path])
            return 'Unstaged changes.'
          }
          await api.gitStage([c.path])
          return 'Staged changes.'
        }),
      },
      { kind: 'item', label: 'Discard changes', enabled: !busy && !c.staged, run: () => void discard(c.path) },
      // Ignoring a file git already tracks changes nothing, so only new files offer it.
      { kind: 'item', label: 'Add to .gitignore', enabled: !busy && c.status === '?', run: () => void run(async () => { await api.gitIgnore(c.path); return 'Added to .gitignore.' }) },
      { kind: 'item', label: 'Copy path', run: () => copyText(c.path) },
    ]
  }
</script>

<div class="sb-panel scm">
  <div class="sb-head">
    <h2>Source control</h2>
    {#if status}
      <button class="branch" title="Switch branch" onclick={openBranchPicker}>{branch}</button>
    {/if}
    <button class="icon" title={tree ? 'View as list' : 'View as tree'} aria-label={tree ? 'View as list' : 'View as tree'} onclick={toggleTree}>
      <Icon name="folder" size={14} />
    </button>
    <button class="icon" title="Refresh" aria-label="Refresh" onclick={() => void loadGit()} disabled={busy}>
      <Icon name="refresh" size={14} />
    </button>
    <div class="pop-wrap">
      <button class="icon" title="More actions" aria-label="More actions" disabled={!status || busy} onclick={() => (menu = menu === 'more' ? null : 'more')}>
        <Icon name="more" size={14} />
      </button>
      {#if menu === 'more'}
        <div class="pop menu" role="menu" tabindex="-1" onpointerdown={(e) => e.stopPropagation()}>
          <button role="menuitem" class="menu-item" disabled={!status?.hasRemote} onclick={() => void run(async () => firstLine(await api.gitPull()))}>Pull</button>
          <button role="menuitem" class="menu-item" disabled={!status?.hasRemote} onclick={() => void run(async () => firstLine(await api.gitPush()))}>Push</button>
          <button role="menuitem" class="menu-item" disabled={!status?.hasRemote} onclick={() => void run(async () => firstLine(await api.gitFetch() || 'Fetched.'))}>Fetch</button>
          <button role="menuitem" class="menu-item" onclick={() => void addRemote()}>Add remote…</button>
          <button role="menuitem" class="menu-item" onclick={() => void undoCommit()}>Undo last commit</button>
        </div>
      {/if}
    </div>
  </div>

  {#if notRepo}
    <EmptyState icon="branch" title="Not a Git repository" hint="Initialize one to track changes in this project." action="Initialize repository" onaction={() => void initRepo()} />
  {:else}
    {#if outside.length}
      <div class="banner">
        <p>Files outside this project are staged: {outside.join(', ')}. Unstage them in another tool before committing here.</p>
      </div>
    {/if}
    {#if status && !status.hasIgnore && !ignoreDismissed}
      <div class="banner">
        <p>Compiled .rpyc files and saves will show up as changes.</p>
        <div class="banner-actions">
          <button disabled={busy} onclick={() => void run(async () => { await api.gitWriteIgnore(); return "Added a Ren'Py .gitignore." })}>Add Ren'Py .gitignore</button>
          <button onclick={() => (ignoreDismissed = true)}>Dismiss</button>
        </div>
      </div>
    {/if}
    <div class="composer">
      <textarea
        rows="2"
        placeholder={`Message (Ctrl+Enter to commit on "${branch}")`}
        bind:value={message}
        onkeydown={onMessageKey}
        disabled={!status || busy}
        spellcheck="true"
      ></textarea>
      <div class="commit-split">
        <button
          class="icon commit"
          title={commitBlocked ? commitBlock : staged.length ? 'Commit staged changes' : 'Commit all changes in game/'}
          aria-label="Commit"
          onclick={() => void commit(false)}
          disabled={busy || !message.trim() || !status || commitBlocked}
        >
          <Icon name="check" size={14} />
        </button>
        <button class="icon caret" title="Commit options" aria-label="Commit options" disabled={!status || busy} onclick={() => (menu = menu === 'commit' ? null : 'commit')}>
          <Icon name="caret" size={12} />
        </button>
        {#if menu === 'commit'}
          <div class="pop commit-pop menu" role="menu" tabindex="-1" onpointerdown={(e) => e.stopPropagation()}>
            <button role="menuitem" class="menu-item" title={commitBlocked ? commitBlock : ''} disabled={!message.trim() || commitBlocked} onclick={() => void commit(false)}>Commit</button>
            <button role="menuitem" class="menu-item" title={commitBlocked ? commitBlock : ''} disabled={!message.trim() || !status?.hasRemote || commitBlocked} onclick={() => void commitAndPush()}>Commit and Push</button>
            <button role="menuitem" class="menu-item" title={commitBlocked ? commitBlock : ''} disabled={commitBlocked} onclick={() => void commit(true)}>Amend last commit</button>
            <button role="menuitem" class="menu-item" onclick={() => void undoCommit()}>Undo last commit</button>
          </div>
        {/if}
      </div>
    </div>
    {#if status?.hasRemote && !status.upstream}
      <button class="sync-btn" disabled={busy} onclick={() => void run(async () => firstLine(await api.gitPush() || 'Published branch.'))}>Publish Branch</button>
    {:else if status && (status.ahead > 0 || status.behind > 0)}
      <button class="sync-btn" disabled={busy} onclick={() => void sync()}>
        Sync Changes{#if status.behind}<span class="sync-n"><Icon name="arrow-down" size={11} />{status.behind}</span>{/if}{#if status.ahead}<span class="sync-n"><Icon name="arrow-up" size={11} />{status.ahead}</span>{/if}
      </button>
    {/if}
    {#if shownErr}<div class="sb-foot note">{shownErr}</div>{/if}

    <div class="sb-list git">
      {#if !status && !git.err}
        <div class="sb-empty" role="status">
          Reading git status…
          <div class="skeleton"></div>
          <div class="skeleton"></div>
          <div class="skeleton"></div>
        </div>
      {/if}

      {#if staged.length}
        <div class="section">
          <button class="sb-section section-toggle" onclick={() => (stagedOpen = !stagedOpen)}>
            <span class="twist" class:open={stagedOpen} aria-hidden="true"><Icon name="chevron-right" size={12} /></span>
            <span>Staged Changes</span>
            <span class="count">{staged.length}</span>
          </button>
          <button class="icon" title="Unstage all changes" aria-label="Unstage all changes" disabled={busy} onclick={() => void run(async () => { const n = staged.length; await api.gitUnstage(staged.map((c) => c.path)); return filesNote('Unstaged', n) })}>
            <Icon name="minus" size={14} />
          </button>
        </div>
        {#if stagedOpen}
          {#if tree}
            {@render folders(stagedTree, true, 0)}
          {:else}
            {#each stagedRows as c (`s:${c.path}:${c.status}`)}
              {@render fileRow(c, true, 0)}
            {/each}
          {/if}
          {#if staged.length > stagedShown}
            <button class="more" onclick={() => (stagedShown += ROW_STEP)}>Show {Math.min(ROW_STEP, staged.length - stagedShown)} more of {staged.length - stagedShown}</button>
          {/if}
        {/if}
      {/if}

      {#if status && (unstaged.length || !staged.length)}
        <div class="section">
          <button class="sb-section section-toggle" onclick={() => (changesOpen = !changesOpen)}>
            <span class="twist" class:open={changesOpen} aria-hidden="true"><Icon name="chevron-right" size={12} /></span>
            <span>Changes</span>
            <span class="count">{unstaged.length}</span>
          </button>
          {#if unstaged.length}
            <button class="icon" title="Stage all changes" aria-label="Stage all changes" disabled={busy} onclick={() => void run(async () => { const n = unstaged.length; await api.gitStage(unstaged.map((c) => c.path)); return filesNote('Staged', n) })}>
              <Icon name="plus" size={14} />
            </button>
          {/if}
        </div>
        {#if changesOpen}
          {#if unstaged.length}
            {#if tree}
              {@render folders(changeTree, false, 0)}
            {:else}
              {#each changeRows as c (`u:${c.path}:${c.status}`)}
                {@render fileRow(c, false, 0)}
              {/each}
            {/if}
            {#if unstaged.length > changesShown}
              <button class="more" onclick={() => (changesShown += ROW_STEP)}>Show {Math.min(ROW_STEP, unstaged.length - changesShown)} more of {unstaged.length - changesShown}</button>
            {/if}
          {:else if !staged.length}
            <EmptyState tone="ok" icon="check" title="No changes" hint="The working tree matches the last commit." />
          {/if}
        {/if}
      {/if}

      <div class="section" bind:this={timelineEl}>
        <button class="sb-section section-toggle" onclick={() => (timelineOpen = !timelineOpen)}>
          <span class="twist" class:open={timelineOpen} aria-hidden="true"><Icon name="chevron-right" size={12} /></span>
          <span>Timeline{timelineName ? ` · ${timelineName}` : ''}</span>
          <span class="count">{timeline.length}</span>
        </button>
      </div>
      {#if timelineOpen}
        {#if !timelineFile}
          <div class="sb-empty">Open a script to see its history.</div>
        {:else}
          {#each timeline as commit (commit.hash)}
            <button class="commit-row" onclick={() => openDiff(`game/${timelineFile}`, `${commit.hash}^`)}>
              <span class="commit-text">
                <span class="sb-name">{commit.subject || commit.hash.slice(0, 7)}</span>
                <span class="sb-meta">{commit.author} · {commit.date} · {commit.hash.slice(0, 7)}</span>
              </span>
            </button>
          {:else}
            <div class="sb-empty">No history for this file.</div>
          {/each}
        {/if}
      {/if}

      {#if commits.length}
        <div class="section">
          <button class="sb-section section-toggle" onclick={() => (historyOpen = !historyOpen)}>
            <span class="twist" class:open={historyOpen} aria-hidden="true"><Icon name="chevron-right" size={12} /></span>
            <span>Commits</span>
            <span class="count">{commits.length}</span>
          </button>
        </div>
        {#if historyOpen}
          {#each commits as commit (commit.hash)}
            <button class="commit-row" class:on={openHash === commit.hash} onclick={() => void toggleCommit(commit.hash)}>
              <span class="twist" class:open={openHash === commit.hash} aria-hidden="true"><Icon name="chevron-right" size={12} /></span>
              <span class="commit-text">
                <span class="sb-name">{commit.subject || commit.hash.slice(0, 7)}</span>
                <span class="sb-meta">{commit.author} · {commit.date} · {commit.hash.slice(0, 7)}</span>
              </span>
            </button>
            {#if openHash === commit.hash}
              {#each commitFiles as file (`${commit.hash}:${file.path}`)}
                {@const parts = fileParts(file.path)}
                <button class="file nested" onclick={() => openDiff(file.path, `${commit.hash}^`)}>
                  <span class="file-name">{parts.name}</span>
                  {#if parts.dir}<span class="file-dir">{parts.dir}</span>{/if}
                  <span class="letter {statusClass(file.status)}">{statusLetter(file.status)}</span>
                </button>
              {:else}
                <div class="sb-empty">No files in this commit.</div>
              {/each}
            {/if}
          {/each}
        {/if}
      {/if}
    </div>
  {/if}
</div>

{#snippet folders(nodes: GitFolder[], isStaged: boolean, depth: number)}
  {#each nodes as node (node.path)}
    {@const shut = closedFolders.has(node.path)}
    {@const under = filesUnder(node)}
    {#if node.name}
    <div class="file folder" style={`padding-left:${depth * 12}px`}>
      <button class="file-main" onclick={() => toggleFolder(node.path)}>
        <span class="twist" class:open={!shut} aria-hidden="true"><Icon name="chevron-right" size={12} /></span>
        <span class="file-name">{node.name}</span>
        <span class="file-dir">{under.length}</span>
      </button>
      <span class="actions">
        {#if isStaged}
          <button class="icon" title="Unstage all changes" aria-label="Unstage all changes" disabled={busy} onclick={() => void run(async () => { await api.gitUnstage(under.map((c) => c.path)); return filesNote('Unstaged', under.length) })}>
            <Icon name="minus" size={14} />
          </button>
        {:else}
          <button class="icon" title="Discard all changes" aria-label="Discard all changes" disabled={busy} onclick={() => void discardMany(under)}>
            <Icon name="undo" size={14} />
          </button>
          <button class="icon" title="Stage all changes" aria-label="Stage all changes" disabled={busy} onclick={() => void run(async () => { await api.gitStage(under.map((c) => c.path)); return filesNote('Staged', under.length) })}>
            <Icon name="plus" size={14} />
          </button>
        {/if}
      </span>
    </div>
    {/if}
    {#if !node.name || !shut}
      {@render folders(node.children, isStaged, node.name ? depth + 1 : depth)}
      {#each node.files as c (`${node.path}:${c.path}:${c.status}`)}
        {@render fileRow(c, isStaged, node.name ? depth + 1 : depth)}
      {/each}
    {/if}
  {/each}
{/snippet}

{#snippet fileRow(c: GitChange, isStaged: boolean, depth: number)}
  {@const parts = fileParts(c.path)}
  <div class="file" style={`padding-left:${depth * 12}px`} role="group" oncontextmenu={(e) => openContextMenu(e, fileMenu(c))}>
    <button class="file-main" title={c.path} onclick={() => openDiff(c.path, diffRev(c))}>
      <span class="file-name">{parts.name}</span>
      {#if parts.dir}<span class="file-dir">{parts.dir}</span>{/if}
    </button>
    <span class="actions">
      <button class="icon" title="Open file" aria-label="Open file" onclick={() => openFile(c.path)}>
        <Icon name="file" size={14} />
      </button>
      {#if !isStaged}
        <button class="icon" title="Discard changes" aria-label="Discard changes" disabled={busy} onclick={() => void discard(c.path)}>
          <Icon name="undo" size={14} />
        </button>
      {/if}
      {#if isStaged}
        <button class="icon" title="Unstage changes" aria-label="Unstage changes" disabled={busy} onclick={() => void run(async () => { await api.gitUnstage([c.path]); return 'Unstaged changes.' })}>
          <Icon name="minus" size={14} />
        </button>
      {:else}
        <button class="icon" title="Stage changes" aria-label="Stage changes" disabled={busy} onclick={() => void run(async () => { await api.gitStage([c.path]); return 'Staged changes.' })}>
          <Icon name="plus" size={14} />
        </button>
      {/if}
    </span>
    <span class="letter {statusClass(c.status)}">{statusLetter(c.status)}</span>
  </div>
{/snippet}

<style>
  .scm { min-height: 0; }
  .branch {
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    border: none;
    background: transparent;
    color: var(--text);
    font-size: var(--fs-md);
    font-weight: 600;
    padding: 2px 6px;
  }
  .pop-wrap, .commit-split { position: relative; display: flex; align-items: flex-start; }
  .pop {
    position: absolute;
    top: 26px;
    right: 0;
    z-index: var(--z-pop);
    min-width: 180px;
  }
  .commit-pop { top: 30px; }
  .composer {
    display: flex;
    align-items: flex-start;
    gap: 2px;
    margin: 8px 8px 4px;
    padding: 2px;
    border: 1px solid var(--line);
    border-radius: var(--r-md);
    background: var(--bg);
  }
  .composer:focus-within { border-color: var(--accent); box-shadow: 0 0 0 1px var(--focus-ring); }
  textarea {
    flex: 1;
    min-width: 0;
    min-height: 44px;
    max-height: 120px;
    resize: vertical;
    border: none;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-md);
    line-height: 1.4;
    padding: 4px 6px;
  }
  textarea:focus { outline: none; }
  .commit { margin-top: 2px; color: var(--text); }
  .commit:not(:disabled) { color: var(--ok); }
  .sync-btn {
    margin: 0 8px 6px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
  }
  .sync-n { display: inline-flex; align-items: center; gap: 1px; }
  .banner {
    margin: 8px 8px 0;
    padding: 8px;
    border: 1px solid var(--line);
    border-radius: var(--r-md);
    background: color-mix(in srgb, var(--warning) 12%, var(--panel));
  }
  .banner p { margin: 0 0 8px; font-size: var(--fs-md); color: var(--text); }
  .banner-actions { display: flex; gap: 6px; flex-wrap: wrap; }
  .banner-actions button { font-size: var(--fs-md); }
  .git { overflow: auto; }
  .more {
    width: 100%;
    border: none;
    border-radius: 0;
    background: transparent;
    color: var(--accent);
    font-size: var(--fs-sm);
    text-align: left;
    padding: 4px 12px;
  }
  .more:hover { background: var(--hover); }
  .section {
    display: flex;
    align-items: center;
    position: sticky;
    top: 0;
    z-index: var(--z-base);
    background: var(--panel);
    border-bottom: 1px solid var(--line);
  }
  .section-toggle {
    flex: 1;
    width: auto;
    min-width: 0;
    border-top: none;
    height: var(--h-control);
  }
  .count { font-weight: 600; letter-spacing: 0; text-transform: none; }
  .file { display: flex; align-items: center; min-height: 22px; padding-right: 6px; }
  .file:hover, .file:focus-within { background: var(--hover); }
  .file-main, .file.nested {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: baseline;
    gap: 8px;
    height: 22px;
    padding: 0 4px 0 8px;
    border: none;
    background: transparent;
    text-align: left;
    color: var(--text);
  }
  .file.nested { width: 100%; padding-left: 34px; }
  .file-name { font-size: var(--fs-md); font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .file-dir { font-size: var(--fs-sm); color: var(--dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .actions { display: none; align-items: center; flex: none; }
  .file:hover .actions, .file:focus-within .actions { display: flex; }
  .file:hover .letter, .file:focus-within .letter { display: none; }
  .letter { flex: none; width: 16px; text-align: center; font-size: var(--fs-sm); font-weight: 700; }
  .letter.modified { color: var(--warning); }
  .letter.added { color: var(--ok); }
  .letter.deleted { color: var(--error); }
  .letter.renamed { color: var(--accent); }
  .commit-row {
    display: flex;
    align-items: flex-start;
    gap: 4px;
    width: 100%;
    padding: 4px 8px 4px 6px;
    border: none;
    border-left: 3px solid transparent;
    border-radius: 0;
    background: transparent;
    text-align: left;
    color: var(--text);
  }
  .commit-row:hover { background: var(--hover); border-color: transparent; }
  .commit-row.on, .commit-row.on:hover { background: var(--sel); }
  .commit-row.on, .commit-row.on:hover { border-left-color: var(--accent); }
  .commit-text { min-width: 0; display: flex; flex-direction: column; gap: 1px; }
  .note { white-space: pre-wrap; }
</style>
