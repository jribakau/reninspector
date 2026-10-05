<script lang="ts">
  import { api, errorText } from '../lib/api'
  import { copyText } from '../lib/context.svelte'
  import { app, bakePatch } from '../lib/store.svelte'
  import type { ArchiveDigest, ModToggles } from '../lib/types'

  const game = $derived(app.info?.game)
  let hashes = $state<ArchiveDigest[]>([])
  let hashError = $state('')
  let hashing = $state(false)
  let toggles = $state<ModToggles>({
    console: false,
    developer: false,
    quickSaveKeys: false,
    skipUnseen: false,
    rollback: false,
  })
  let toggleError = $state('')

  $effect(() => {
    const root = app.info?.root
    if (!root) return
    let cancel = false
    void api.modTogglesGet().then(
      (got) => {
        if (!cancel) toggles = got
      },
      (e) => {
        if (!cancel) toggleError = errorText(e)
      },
    )
    return () => {
      cancel = true
    }
  })

  async function setToggle(key: keyof ModToggles, on: boolean) {
    const next = { ...toggles, [key]: on }
    toggles = next
    toggleError = ''
    try {
      await api.modTogglesSet(next)
    } catch (e) {
      toggleError = errorText(e)
    }
  }

  $effect(() => {
    const root = app.info?.root
    const count = app.info?.game.archives.length ?? 0
    hashes = []
    hashError = ''
    if (!root || !count) return
    let cancel = false
    hashing = true
    void api.archiveFingerprints().then(
      (list) => {
        if (!cancel) {
          hashes = list
          hashing = false
        }
      },
      (e) => {
        if (!cancel) {
          hashError = errorText(e)
          hashing = false
        }
      },
    )
    return () => {
      cancel = true
    }
  })

  function shown(path: string): ArchiveDigest | undefined {
    return hashes.find((h) => h.path === path)
  }

  function fingerprintText(): string {
    if (!game) return ''
    const lines = [
      game.name ?? app.info?.name ?? '',
      game.version ? `version ${game.version}` : '',
      game.engineVersion ? `Ren'Py ${game.engineVersion}` : '',
      game.scriptVersion && game.scriptVersion !== game.engineVersion
        ? `scripts ${game.scriptVersion}`
        : '',
    ].filter(Boolean)
    for (const archive of game.archives) {
      const hash = shown(archive.path)?.sha256
      lines.push(`${archive.path} ${hash ?? '(not hashed)'}`)
    }
    return lines.join('\n')
  }
</script>

<div class="sb-panel game">
  {#if game}
    <dl>
      <dt>Name</dt>
      <dd>{game.name ?? app.info?.name ?? '—'}</dd>
      <dt>Game version</dt>
      <dd>{game.version ?? '—'}</dd>
      <dt>Ren'Py</dt>
      <dd>{game.engineVersion ?? 'unknown'}</dd>
      {#if game.scriptVersion && game.scriptVersion !== game.engineVersion}
        <dt>Scripts written for</dt>
        <dd>{game.scriptVersion}</dd>
      {/if}
      <dt>Build name</dt>
      <dd>{game.buildName ?? '—'}</dd>
      <dt>Save directory</dt>
      <dd>{game.saveDirectory ?? '—'}</dd>
      <dt>Archive files dated</dt>
      <dd>
        {#if game.filesOldest}
          {game.filesOldest} – {game.filesNewest}
          <span class="sb-meta">Modification times, not a build date.</span>
        {:else}
          —
        {/if}
      </dd>
    </dl>

    <div class="note">
      <div>Layout</div>
      <span class="sb-meta">
        {game.layout.looseScripts} loose · {game.layout.overrideScripts} overriding ·
        {game.layout.archivedScripts} archived · {game.layout.compiledScripts} decompiled ·
        {game.layout.compiledOnly} compiled-only
      </span>
      <span class="sb-meta">
        {game.layout.archives} archive{game.layout.archives === 1 ? '' : 's'}
        {#if game.layout.nestedArchives} · {game.layout.nestedArchives} nested{/if}
        {#if game.layout.formats.length}
          · {game.layout.formats.map((f) => `${f.count} ${f.version}`).join(', ')}
        {/if}
      </span>
    </div>

    {#each game.notes as note (note)}
      <p class="warn">{note}</p>
    {/each}

    <div class="note">
      <div>Mod toggles</div>
      <span class="sb-meta">Bake to apply. They are written as vnide_toggles.rpy inside the patch.</span>
      <label><input type="checkbox" checked={toggles.console} onchange={(e) => setToggle('console', e.currentTarget.checked)} /> Console</label>
      <label><input type="checkbox" checked={toggles.developer} onchange={(e) => setToggle('developer', e.currentTarget.checked)} /> Developer</label>
      <label><input type="checkbox" checked={toggles.quickSaveKeys} onchange={(e) => setToggle('quickSaveKeys', e.currentTarget.checked)} /> Quick save on F5, quick load on F9</label>
      <label><input type="checkbox" checked={toggles.skipUnseen} onchange={(e) => setToggle('skipUnseen', e.currentTarget.checked)} /> Skip unseen text</label>
      <label><input type="checkbox" checked={toggles.rollback} onchange={(e) => setToggle('rollback', e.currentTarget.checked)} /> Rollback</label>
      {#if toggleError}<p class="warn">{toggleError}</p>{/if}
      <button type="button" onclick={() => bakePatch({ toggles: true })} disabled={!!app.busy}>
        Bake to apply
      </button>
    </div>

    {#if game.archives.length}
      <div class="note">
        <div class="row">
          <span>Archive fingerprints</span>
          <button type="button" onclick={() => copyText(fingerprintText())} disabled={hashing}>Copy</button>
        </div>
        {#if hashing}<span class="sb-meta">Hashing…</span>{/if}
        {#if hashError}<p class="warn">{hashError}</p>{/if}
        {#each game.archives as archive (archive.path)}
          <div class="fp">
            <span class="sb-name">{archive.path}</span>
            <span class="sb-meta">{shown(archive.path)?.sha256 ?? (hashing ? '…' : 'unavailable')}</span>
          </div>
        {/each}
      </div>
    {/if}
  {:else}
    <p class="sb-empty">Open a project to see its version and layout.</p>
  {/if}
</div>

<style>
  .game {
    overflow: auto;
    padding: 4px 0 12px;
  }
  dl {
    display: grid;
    grid-template-columns: 9.5rem 1fr;
    gap: 4px 8px;
    margin: 0 8px 8px;
    font-size: var(--fs-md);
  }
  dt {
    color: var(--dim);
  }
  dd {
    margin: 0;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .note {
    margin: 0 8px 8px;
    padding: 8px;
    border: 1px solid var(--line);
    border-radius: var(--r-md);
    font-size: var(--fs-md);
    display: grid;
    gap: 6px;
  }
  .row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
  }
  .fp {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .fp .sb-meta {
    font-family: var(--mono);
    overflow-wrap: anywhere;
  }
  label {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .warn {
    margin: 0 8px 8px;
    font-size: var(--fs-md);
    color: var(--warning);
  }
</style>
