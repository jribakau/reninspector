import { hasEditor, requestSave, revertFile, runEditor, createScript } from './edit.svelte'
import { check } from '@tauri-apps/plugin-updater'
import { api, errorText } from './api'
import { openBuildDialog } from './build.svelte'
import {
  checkWithEngine,
  runEngineLint,
  toggleAutoreload,
} from './engine.svelte'
import { openSdkManager } from './sdk.svelte'
import { flowPane } from './flowops.svelte'
import { sceneRedo, sceneUndo } from './scene.svelte'
import { notify } from './toast.svelte'
import { jumpGameHere, replayToCursor, runGame, toggleLive } from './live.svelte'
import { forgetTrust, isTrusted } from './trust.svelte'
import { app } from './model.svelte'
import {
  canGoBack,
  canGoForward,
  canReopenClosed,
  closeActiveTab,
  cycleTabs,
  navBack,
  navForward,
  nextProblem,
  openActivity,
  openBottom,
  openMapTab,
  toggleBottom,
  toggleScene,
  toggleFlow,
  toggleStage,
  reopenClosedTab,
  saveAll,
  toggleFlowDetail,
  toggleSidebar,
} from './nav.svelte'
import { openSettings } from './settings.svelte'
import { appearance, bumpFont, chooseAndOpenProject, createNewProject, toggleSpell, toggleTheme, toggleWrap } from './project.svelte'

export interface Action {
  id: string
  label: string
  shortcut?: string
  title?: string
  palette: boolean
  enabled: boolean
  checked: boolean
  run: () => void
}

export type MenuNode = { kind: 'sep' } | { kind: 'recent' } | { kind: 'action'; action: Action }

export interface MenuSection {
  id: string
  label: string
  access: string
  items: MenuNode[]
}

/** Bumped from the window key handler so the shell can open a menu. */
export const menuRequest = $state({ letter: '', seq: 0 })

export function requestMenu(letter: string) {
  menuRequest.letter = letter
  menuRequest.seq += 1
}

function action(
  partial: {
    id: string
    label: string
    run: () => void
    shortcut?: string
    title?: string
    palette?: boolean
    enabled?: boolean
    checked?: boolean
  },
): Action {
  return {
    palette: partial.palette ?? true,
    enabled: partial.enabled ?? true,
    checked: partial.checked ?? false,
    id: partial.id,
    label: partial.label,
    shortcut: partial.shortcut,
    title: partial.title,
    run: partial.run,
  }
}

/** File, Edit, View, Go, Run, and Help. The command palette uses the same actions. */
export function menuSections(): MenuSection[] {
  const project = !!app.info
  const busy = !!app.busy
  const editor = hasEditor()
  const warpOff = app.live.running && !app.live.canWarp
  const file = app.loc?.file

  const runTitle = app.launcher ?? app.info?.launcher ?? 'Auto-detect launcher'
  const warpTitle = warpOff
    ? 'This session cannot warp to a line. Stop live, or use Replay to line.'
    : 'Run from here: open the game at this line. Earlier dialogue is skipped. Scenes on the way are rebuilt, and a name that was never set is treated as false, so an if may take the other branch. Replay to line runs the story instead.'

  return [
    {
      id: 'file',
      label: 'File',
      access: 'f',
      items: [
        { kind: 'action', action: action({ id: 'new-file', label: 'New script', enabled: project && !busy, run: () => void createScript() }) },
        { kind: 'action', action: action({ id: 'new-project', label: 'New project', enabled: !busy, run: () => void createNewProject() }) },
        { kind: 'action', action: action({ id: 'open-project', label: 'Open project…', shortcut: '', enabled: !busy, run: () => void chooseAndOpenProject() }) },
        { kind: 'recent' },
        { kind: 'sep' },
        { kind: 'action', action: action({ id: 'save', label: 'Save', shortcut: 'Ctrl+S', enabled: editor && !busy, run: requestSave }) },
        { kind: 'action', action: action({ id: 'save-all', label: 'Save all', shortcut: 'Ctrl+Alt+S', enabled: project && !busy && app.dirtyFiles.length > 0, run: () => void saveAll() }) },
        { kind: 'action', action: action({ id: 'reopen', label: 'Reopen closed editor', shortcut: 'Ctrl+Shift+T', enabled: project && canReopenClosed(), run: reopenClosedTab }) },
        { kind: 'action', action: action({ id: 'close-tab', label: 'Close editor', shortcut: 'Ctrl+W', enabled: !!app.activeEditor, run: closeActiveTab }) },
        { kind: 'sep' },
        {
          kind: 'action',
          action: action({
            id: 'revert',
            label: 'Revert file',
            enabled: editor && !busy && !!file,
            run: () => {
              if (file) void revertFile(file)
            },
          }),
        },
        { kind: 'sep' },
        { kind: 'action', action: action({ id: 'settings', label: 'Settings…', shortcut: 'Ctrl+,', title: 'Editor, saving, appearance, and project options', run: () => openSettings() }) },
      ],
    },
    {
      id: 'edit',
      label: 'Edit',
      access: 'e',
      items: [
        { kind: 'action', action: action({ id: 'undo', label: 'Undo', shortcut: 'Ctrl+Z', enabled: flowPane.active ? app.sceneCanUndo : editor, run: () => (flowPane.active ? void sceneUndo() : runEditor('undo')) }) },
        { kind: 'action', action: action({ id: 'redo', label: 'Redo', shortcut: 'Ctrl+Y', enabled: flowPane.active ? app.sceneCanRedo : editor, run: () => (flowPane.active ? void sceneRedo() : runEditor('redo')) }) },
        { kind: 'sep' },
        { kind: 'action', action: action({ id: 'find', label: 'Find', shortcut: 'Ctrl+F', enabled: editor, run: () => runEditor('find') }) },
        { kind: 'action', action: action({ id: 'replace', label: 'Replace', shortcut: 'Ctrl+H', enabled: editor, run: () => runEditor('replace') }) },
        { kind: 'action', action: action({ id: 'goto-line', label: 'Go to line', shortcut: 'Ctrl+G', enabled: editor, run: () => runEditor('gotoLine') }) },
        { kind: 'action', action: action({ id: 'select-all', label: 'Select all', shortcut: 'Ctrl+A', enabled: editor, run: () => runEditor('selectAll') }) },
        { kind: 'action', action: action({ id: 'comment', label: 'Toggle comment', shortcut: 'Ctrl+/', enabled: editor, run: () => runEditor('toggleComment') }) },
        { kind: 'action', action: action({ id: 'select-next', label: 'Add selection to next match', shortcut: 'Ctrl+D', enabled: editor, run: () => runEditor('selectNext') }) },
        { kind: 'action', action: action({ id: 'move-line-up', label: 'Move line up', shortcut: 'Alt+Up', enabled: editor, run: () => runEditor('moveLineUp') }) },
        { kind: 'action', action: action({ id: 'move-line-down', label: 'Move line down', shortcut: 'Alt+Down', enabled: editor, run: () => runEditor('moveLineDown') }) },
        { kind: 'action', action: action({ id: 'fold', label: 'Fold', shortcut: 'Ctrl+Shift+[', enabled: editor, run: () => runEditor('fold') }) },
        { kind: 'action', action: action({ id: 'unfold', label: 'Unfold', shortcut: 'Ctrl+Shift+]', enabled: editor, run: () => runEditor('unfold') }) },
        { kind: 'action', action: action({ id: 'format', label: 'Format document', shortcut: 'Shift+Alt+F', enabled: editor, run: () => runEditor('format') }) },
        { kind: 'action', action: action({ id: 'format-selection', label: 'Format selection', enabled: editor, run: () => runEditor('formatSelection') }) },
        { kind: 'sep' },
        { kind: 'action', action: action({ id: 'goto', label: 'Go to definition', shortcut: 'F12', enabled: editor, run: () => runEditor('goto') }) },
        { kind: 'action', action: action({ id: 'refs', label: 'Find references', shortcut: 'Shift+F12', enabled: editor, run: () => runEditor('refs') }) },
        { kind: 'action', action: action({ id: 'rename', label: 'Rename symbol', shortcut: 'F2', enabled: editor, run: () => runEditor('rename') }) },
      ],
    },
    {
      id: 'view',
      label: 'View',
      access: 'v',
      items: [
        { kind: 'action', action: action({ id: 'palette', label: 'Command palette', shortcut: 'Ctrl+Shift+P', run: () => { app.palette = 'commands' } }) },
        { kind: 'sep' },
        { kind: 'action', action: action({ id: 'explorer', label: 'Explorer', enabled: project, checked: project && app.sidebarOpen && app.activity === 'explorer', run: () => openActivity('explorer') }) },
        { kind: 'action', action: action({ id: 'story', label: 'Story', enabled: project, checked: project && app.sidebarOpen && app.activity === 'story', run: () => openActivity('story') }) },
        { kind: 'action', action: action({ id: 'search', label: 'Search', shortcut: 'Ctrl+Shift+F', enabled: project, checked: project && app.sidebarOpen && app.activity === 'search', run: () => openActivity('search') }) },
        { kind: 'action', action: action({ id: 'renpy', label: "Ren'Py", enabled: project, checked: project && app.sidebarOpen && app.activity === 'renpy', run: () => openActivity('renpy') }) },
        { kind: 'action', action: action({ id: 'git', label: 'Git', enabled: project, checked: project && app.sidebarOpen && app.activity === 'git', run: () => openActivity('git') }) },
        { kind: 'sep' },
        { kind: 'action', action: action({ id: 'problems', label: 'Problems', enabled: project, checked: project && app.bottomOpen && app.bottomTab === 'problems', run: () => openBottom('problems') }) },
        { kind: 'action', action: action({ id: 'console', label: 'Game log', title: "The game's log.txt, traceback.txt and errors.txt", enabled: project, checked: project && app.bottomOpen && app.bottomTab === 'log', run: () => openBottom('log') }) },
        { kind: 'action', action: action({ id: 'events', label: 'App log', title: "What Ren'Inspector did: opens, saves, builds and errors", checked: app.bottomOpen && app.bottomTab === 'events', run: () => openBottom('events') }) },
        { kind: 'action', action: action({ id: 'live', label: 'Live', enabled: project, checked: project && app.bottomOpen && app.bottomTab === 'live', run: () => openBottom('live') }) },
        { kind: 'action', action: action({ id: 'build-panel', label: 'Build output', enabled: project, checked: project && app.bottomOpen && app.bottomTab === 'build', run: () => openBottom('build') }) },
        { kind: 'sep' },
        { kind: 'action', action: action({ id: 'sidebar', label: 'Sidebar', shortcut: 'Ctrl+B', enabled: project, checked: project && app.sidebarOpen, run: toggleSidebar }) },
        { kind: 'action', action: action({ id: 'bottom', label: 'Bottom panel', shortcut: 'Ctrl+J', enabled: project, checked: project && app.bottomOpen, run: toggleBottom }) },
        { kind: 'action', action: action({ id: 'stage', label: 'Stage preview', shortcut: 'Ctrl+Shift+V', enabled: project, checked: project && app.stageOpen, run: toggleStage }) },
        { kind: 'action', action: action({ id: 'flow', label: 'Flow graph', shortcut: 'Ctrl+Shift+L', enabled: project, checked: project && app.flowOpen, run: toggleFlow }) },
        { kind: 'action', action: action({ id: 'detail', label: 'Detailed flow', enabled: project, checked: project && app.flowDetail, run: toggleFlowDetail }) },
        { kind: 'action', action: action({ id: 'scene', label: 'Flow in center', shortcut: 'Ctrl+E', enabled: project && !!app.activeEditor?.startsWith('file:'), checked: project && app.sceneExpanded && !!app.activeEditor?.startsWith('file:'), run: toggleScene }) },
        { kind: 'action', action: action({ id: 'map', label: 'Project map', enabled: project, run: openMapTab }) },
        { kind: 'sep' },
        { kind: 'action', action: action({ id: 'theme', label: 'Light theme', checked: appearance.light, run: toggleTheme }) },
        { kind: 'action', action: action({ id: 'wrap', label: 'Word wrap', shortcut: 'Alt+Z', checked: appearance.wrap, run: toggleWrap }) },
        { kind: 'action', action: action({ id: 'spell', label: 'Spell check', checked: appearance.spell, enabled: project, title: 'Underlines words in dialogue and menu choices that are not in the English dictionary.', run: toggleSpell }) },
        { kind: 'action', action: action({ id: 'font-up', label: 'Increase editor font', run: () => bumpFont(1) }) },
        { kind: 'action', action: action({ id: 'font-down', label: 'Decrease editor font', run: () => bumpFont(-1) }) },
      ],
    },
    {
      id: 'go',
      label: 'Go',
      access: 'g',
      items: [
        { kind: 'action', action: action({ id: 'back', label: 'Back', shortcut: 'Alt+Left', enabled: project && canGoBack(), run: navBack }) },
        { kind: 'action', action: action({ id: 'forward', label: 'Forward', shortcut: 'Alt+Right', enabled: project && canGoForward(), run: navForward }) },
        { kind: 'sep' },
        { kind: 'action', action: action({ id: 'goto-file', label: 'Go to file', shortcut: 'Ctrl+P', enabled: project, run: () => { app.palette = 'files' } }) },
        { kind: 'action', action: action({ id: 'goto-symbol', label: 'Go to symbol', shortcut: 'Ctrl+T', enabled: project, run: () => { app.palette = 'symbols' } }) },
        { kind: 'sep' },
        { kind: 'action', action: action({ id: 'next-problem', label: 'Next problem', shortcut: 'F8', enabled: project, run: () => nextProblem(1) }) },
        { kind: 'action', action: action({ id: 'prev-problem', label: 'Previous problem', shortcut: 'Shift+F8', enabled: project, run: () => nextProblem(-1) }) },
        { kind: 'sep' },
        { kind: 'action', action: action({ id: 'next-tab', label: 'Next editor', shortcut: 'Ctrl+Tab', enabled: app.editorTabs.length > 1, run: () => cycleTabs(1) }) },
        { kind: 'action', action: action({ id: 'prev-tab', label: 'Previous editor', shortcut: 'Ctrl+Shift+Tab', enabled: app.editorTabs.length > 1, run: () => cycleTabs(-1) }) },
      ],
    },
    {
      id: 'run',
      label: 'Run',
      access: 'r',
      items: [
        { kind: 'action', action: action({ id: 'run-game', label: 'Run game', enabled: project && !busy, title: runTitle, run: () => void runGame() }) },
        { kind: 'action', action: action({ id: 'live-jump', label: 'Run from here', shortcut: 'Ctrl+F5', enabled: project && !busy && !warpOff, title: warpTitle, run: () => void jumpGameHere() }) },
        {
          kind: 'action',
          action: action({
            id: 'live-replay',
            label: 'Replay to line',
            shortcut: 'Ctrl+Shift+F5',
            enabled: project && !busy,
            title: 'Run the game from the start and stop at the selected line. Menus on the way are answered from the script. An if still depends on the game. Persistent data can change.',
            run: () => void replayToCursor(),
          }),
        },
        {
          kind: 'action',
          action: action({
            id: 'live-start',
            label: app.live.running ? 'Stop live' : 'Start live',
            shortcut: 'F5',
            enabled: project && !busy,
            title: 'Start the game in its own window and follow the line it is on. Stop closes that session.',
            run: () => void toggleLive(),
          }),
        },
        { kind: 'sep' },
        {
          kind: 'action',
          action: action({
            id: 'lint',
            label: 'Run lint',
            enabled: project && !busy,
            title: 'Runs Ren\'Py\'s own lint and adds its findings to Problems (same safeguards as the engine check).',
            run: () => void runEngineLint(),
          }),
        },
        {
          kind: 'action',
          action: action({
            id: 'engine',
            label: app.info?.engine ? 'Re-check with engine' : 'Check with engine',
            enabled: project && !busy,
            title: 'Runs the game\'s own Ren\'Py engine headless to list every label and screen it loads. This executes the game\'s init code; log.txt, cache and saves are backed up and restored.',
            run: () => void checkWithEngine(),
          }),
        },
        {
          kind: 'action',
          action: action({
            id: 'forget-trust',
            label: 'Stop trusting this game',
            enabled: project && !busy && isTrusted(),
            title: 'Ask again before running, previewing, checking or building this game, since all of them run its code.',
            run: () => forgetTrust(),
          }),
        },
        {
          kind: 'action',
          action: action({
            id: 'launcher',
            label: 'Ren\'Py SDKs…',
            enabled: !busy,
            title: 'Download, add, or choose the Ren\'Py version used to run and build',
            run: () => void openSdkManager(),
          }),
        },
        {
          kind: 'action',
          action: action({
            id: 'reload',
            label: 'Auto-reload',
            enabled: project && !busy,
            checked: app.autoreload,
            title: 'Writes game/vnide_autoreload.rpy so the next launch reloads scripts after a save. Restart the game once to pick it up.',
            run: () => void toggleAutoreload(),
          }),
        },
        {
          kind: 'action',
          action: action({
            id: 'build',
            label: 'Build…',
            enabled: project && !busy,
            title: 'Package the game for Windows and Linux, Mac, or the web',
            run: () => openBuildDialog(),
          }),
        },
      ],
    },
    {
      id: 'help',
      label: 'Help',
      access: 'h',
      items: [
        { kind: 'action', action: action({ id: 'palette', label: 'Command palette', shortcut: 'Ctrl+Shift+P', run: () => { app.palette = 'commands' } }) },
        {
          kind: 'action',
          action: action({
            id: 'check-update',
            label: 'Check for updates',
            title: 'Looks for a newer release on GitHub and installs it. Restart the app afterwards.',
            run: () => void checkForUpdates(),
          }),
        },
        {
          kind: 'action',
          action: action({
            id: 'diagnostics-copy',
            label: 'Copy diagnostic bundle',
            title: 'Copies the app version, system, and the open project\'s recent log lines. Nothing is sent anywhere.',
            run: () => void copyDiagnostics(),
          }),
        },
        {
          kind: 'action',
          action: action({
            id: 'about',
            label: "About Ren'Inspector",
            run: () => {
              app.notice = "Ren'Inspector 0.1.0 — project map and editor for Ren'Py games."
            },
          }),
        },
      ],
    },
  ]
}

async function checkForUpdates() {
  if (app.busy) return
  app.error = ''
  app.busy = 'Checking for updates…'
  try {
    const update = await check()
    if (!update) {
      app.notice = "Ren'Inspector is up to date."
      return
    }
    const dirty = app.dirtyFiles.length
    const go = confirm(
      `Ren'Inspector ${update.version} is available.\n\n` +
        `Installing closes Ren'Inspector${dirty ? ' after saving your unsaved changes' : ''}. ` +
        `It installs the .msi version, so a copy run from the portable zip ends up installed separately.\n\n` +
        `Install now?`,
    )
    if (!go) return
    // The installer ends this process on Windows, so nothing unsaved may be left.
    if (dirty) {
      app.busy = 'Saving…'
      await saveAll({ quiet: true })
      if (app.dirtyFiles.length) {
        app.error = 'Some files could not be saved, so the update was not installed.'
        return
      }
    }
    app.busy = `Downloading ${update.version}…`
    await update.downloadAndInstall()
    notify(`Installed ${update.version}. Restart Ren'Inspector to use it.`, 'ok')
  } catch (e) {
    app.error = errorText(e)
  } finally {
    app.busy = ''
  }
}

async function copyDiagnostics() {
  try {
    const text = await api.diagnosticBundle()
    await navigator.clipboard.writeText(text)
    notify('Copied a diagnostic bundle to the clipboard.', 'ok')
  } catch (e) {
    app.error = errorText(e)
  }
}

export function paletteActions(): Action[] {
  const seen = new Set<string>()
  const out: Action[] = []
  for (const section of menuSections()) {
    for (const item of section.items) {
      if (item.kind !== 'action' || !item.action.palette || seen.has(item.action.id)) continue
      seen.add(item.action.id)
      out.push(item.action)
    }
  }
  return out
}
