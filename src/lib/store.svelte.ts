export type { Activity, BottomTab, EditorTab, LiveView, Loc, PaletteMode, RenpySection } from './model.svelte'
export { editorTabId } from './model.svelte'
export { app } from './model.svelte'

export { diagnosticsFor, fileInfo, fileOfNode, labelAt, lookupSymbol, nodeByName, nodesInFile, symbolsOf } from './indexes.svelte'

export {
  activateEditor,
  canGoBack,
  canGoForward,
  canReopenClosed,
  closeActiveTab,
  closeAllTabs,
  bulkCloseCount,
  closeEditor,
  closeFile,
  closeSavedTabs,
  closeTabsToTheLeft,
  isPinned,
  moveEditor,
  pinEditor,
  unpinEditor,
  cycleTabs,
  cursorMoved,
  followFlowLabel,
  goTo,
  navBack,
  navForward,
  nextProblem,
  openActivity,
  openBottom,
  openDiff,
  openLabelGraph,
  openMapTab,
  openRenpy,
  reopenClosedTab,
  revealInExplorer,
  saveAll,
  searchDialogue,
  selectLabel,
  showActivity,
  showBottom,
  showReferences,
  toggleBottom,
  showCode,
  toggleScene,
  toggleFlow,
  toggleStage,
  toggleFlowDetail,
  toggleSidebar,
} from './nav.svelte'

export {
  applyRename,
  cancelRename,
  confirmDiscard,
  createScript,
  deleteScript,
  impactSummary,
  noteStale,
  hasEditor,
  registerEditor,
  registerSave,
  registerBufferSave,
  renameScript,
  runEditor,
  renameSymbol,
  requestSave,
  revertFile,
  saveFile,
  setDirty,
  reloadEditor,
} from './edit.svelte'

export {
  jumpGameHere,
  jumpLiveLabel,
  playFromCursor,
  reloadLive,
  replayToCursor,
  runGame,
  setWatchVars,
  toggleFollowGame,
  toggleLive,
} from './live.svelte'

export { bakePatch, buildArchive, cancelArchiveJob, extractArchive, removePatch, undoPatch } from './archives.svelte'

export { checkWithEngine, runEngineLint, toggleAutoreload } from './engine.svelte'
export { openBuildDialog } from './build.svelte'
export { openSdkManager } from './sdk.svelte'

export {
  applyFontSize,
  applyTheme,
  bootstrap,
  bumpFont,
  chooseAndOpenProject,
  createNewProject,
  openProject,
  toggleTheme,
} from './project.svelte'
