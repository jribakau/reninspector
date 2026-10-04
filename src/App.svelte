<script lang="ts">
  import { onMount } from 'svelte'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import BranchPicker from './components/BranchPicker.svelte'
  import CommandPalette from './components/CommandPalette.svelte'
import ContextMenu from './components/ContextMenu.svelte'
  import { watchGit } from './lib/git.svelte'
  import { anyModal, handleOverlayKey } from './lib/overlay.svelte'
  import { requestMenu } from './lib/menus.svelte'
  import { toggleWrap } from './lib/project.svelte'
  import { openSettings } from './lib/settings.svelte'
import { app, bootstrap, closeActiveTab, confirmDiscard, cycleTabs, jumpGameHere, navBack, navForward, nextProblem, openActivity, reopenClosedTab, replayToCursor, runEditor, saveAll, showCode, toggleBottom, toggleFlow, toggleLive, toggleScene, toggleSidebar, toggleStage } from './lib/store.svelte'
  import AppHeader from './shell/AppHeader.svelte'
  import AskDialog from './shell/AskDialog.svelte'
  import BuildDialog from './shell/BuildDialog.svelte'
  import SdkManager from './shell/SdkManager.svelte'
  import SettingsDialog from './shell/SettingsDialog.svelte'
  import Banners from './shell/Banners.svelte'
  import ProjectLoading from './shell/ProjectLoading.svelte'
  import RenameDialog from './shell/RenameDialog.svelte'
  import StatusBar from './shell/StatusBar.svelte'
  import Welcome from './shell/Welcome.svelte'
  import Workspace from './shell/Workspace.svelte'

  watchGit()

  function typing(): boolean {
    const el = document.activeElement
    if (!(el instanceof HTMLElement)) return false
    // The code editor stays mounted while the scene covers it, and can keep focus.
    if (el.closest('.pane.hidden')) return false
    return el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.isContentEditable
  }

  function inEditor(): boolean {
    const el = document.activeElement
    return el instanceof HTMLElement && !!el.closest('.cm-editor')
  }

  function paletteKey(e: KeyboardEvent) {
    if (anyModal()) return
    if (e.key === 'Escape') {
      if (e.defaultPrevented || app.palette || typing() || !app.sceneExpanded) return
      e.preventDefault()
      showCode()
      return
    }
    if (e.key === 'F8' && !e.ctrlKey && !e.metaKey && !e.altKey) {
      e.preventDefault()
      nextProblem(e.shiftKey ? -1 : 1)
      return
    }
    if (e.key === 'F5' && !e.altKey && app.info && !app.busy) {
      e.preventDefault()
      if ((e.ctrlKey || e.metaKey) && e.shiftKey) void replayToCursor()
      else if (e.ctrlKey || e.metaKey) void jumpGameHere()
      else if (!e.shiftKey) void toggleLive()
      return
    }
    if (e.altKey && !e.ctrlKey && !e.metaKey && !e.shiftKey && (e.key === 'ArrowLeft' || e.key === 'ArrowRight')) {
      e.preventDefault()
      if (e.key === 'ArrowLeft') navBack()
      else navForward()
      return
    }
    if ((e.ctrlKey || e.metaKey) && e.altKey && !e.shiftKey && e.key.toLowerCase() === 's') {
      e.preventDefault()
      void saveAll()
      return
    }
    if (e.altKey && !e.ctrlKey && !e.metaKey && !e.shiftKey && e.key.length === 1) {
      const letter = e.key.toLowerCase()
      if (letter === 'z') {
        e.preventDefault()
        toggleWrap()
        return
      }
      if (typing()) return
      if ('fevgrh'.includes(letter) && !app.palette) {
        e.preventDefault()
        requestMenu(letter)
      }
      return
    }
    if (!(e.ctrlKey || e.metaKey) || e.altKey) return
    const key = e.key.toLowerCase()
    if (key === 'tab') {
      e.preventDefault()
      cycleTabs(e.shiftKey ? -1 : 1)
    } else if (key === ',' && !e.shiftKey) {
      e.preventDefault()
      openSettings()
    } else if (key === 'w' && !e.shiftKey) {
      e.preventDefault()
      closeActiveTab()
    } else if (key === 'p') {
      e.preventDefault()
      app.palette = e.shiftKey ? 'commands' : 'files'
    } else if (key === 't' && e.shiftKey) {
      e.preventDefault()
      reopenClosedTab()
    } else if (key === 't' && !e.shiftKey) {
      e.preventDefault()
      app.palette = 'symbols'
    } else if (key === 'b' && !e.shiftKey) {
      e.preventDefault()
      toggleSidebar()
    } else if (key === 'j' && !e.shiftKey) {
      e.preventDefault()
      toggleBottom()
    } else if (key === 'g' && !e.shiftKey) {
      if (e.defaultPrevented) return
      e.preventDefault()
      runEditor('gotoLine')
    } else if (key === 'h' && !e.shiftKey) {
      if (e.defaultPrevented) return
      e.preventDefault()
      runEditor('replace')
    } else if (key === 'f' && e.shiftKey && app.info) {
      e.preventDefault()
      openActivity('search')
    } else if (key === 'e' && !e.shiftKey && app.info) {
      e.preventDefault()
      toggleScene()
    } else if (key === 'v' && e.shiftKey && app.info && !inEditor()) {
      e.preventDefault()
      toggleStage()
    } else if (key === 'l' && e.shiftKey && app.info && !inEditor()) {
      e.preventDefault()
      toggleFlow()
    }
  }

  function blockMouseNav(e: MouseEvent) {
    if (e.button === 3 || e.button === 4) e.preventDefault()
  }

  function mouseNav(e: MouseEvent) {
    if (e.button !== 3 && e.button !== 4) return
    e.preventDefault()
    if (anyModal()) return
    if (e.button === 3) navBack()
    else navForward()
  }

  function blockNativeMenu(e: MouseEvent) {
    const target = e.target
    if (target instanceof HTMLElement && target.closest('input, textarea, [contenteditable="true"]')) return
    e.preventDefault()
  }

  onMount(() => {
    window.addEventListener('keydown', handleOverlayKey, true)
    window.addEventListener('keydown', paletteKey)
    window.addEventListener('contextmenu', blockNativeMenu, true)
    window.addEventListener('mousedown', blockMouseNav)
    window.addEventListener('mouseup', mouseNav)
    void bootstrap()
    let unlisten: (() => void) | undefined
    void getCurrentWindow()
      .onCloseRequested((e) => {
        if (!confirmDiscard()) e.preventDefault()
      })
      .then((fn) => {
        unlisten = fn
      })
    return () => {
      window.removeEventListener('keydown', handleOverlayKey, true)
      window.removeEventListener('keydown', paletteKey)
      window.removeEventListener('contextmenu', blockNativeMenu, true)
      window.removeEventListener('mousedown', blockMouseNav)
      window.removeEventListener('mouseup', mouseNav)
      unlisten?.()
    }
  })
</script>

<div class="app">
  <AppHeader />
  <Banners />
  <div class="main">
    {#if !app.info}
      <Welcome />
    {:else}
      <Workspace />
    {/if}
    {#if app.opening}
      <ProjectLoading />
    {/if}
  </div>
  <StatusBar />
  <CommandPalette />
  <BranchPicker />
  <RenameDialog />
  <AskDialog />
  <SettingsDialog />
  <SdkManager />
  <BuildDialog />
  <ContextMenu />
</div>

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100%;
    outline: 1px solid var(--line);
    outline-offset: -1px;
  }
  :global(html.maximized) .app {
    outline-color: transparent;
  }
  .main {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
</style>
