import { invoke as rawInvoke, type InvokeArgs } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { logUi, type AppLogInput } from './applog.svelte'
import { open, save } from '@tauri-apps/plugin-dialog'
import type {
  ChangedPayload,
  DiagReport,
  EditImpact,
  LabelGraph,
  LabelLines,
  SceneReport,
  LaunchReport,
  LiveReport,
  LiveState,
  ArchiveDigest,
  ArchiveEntryInfo,
  PatchReport,
  ModToggles,
  RebaseResult,
  SaveSlot,
  SaveDetail,
  ProjectInfo,
  ProjectMap,
  RpaProgress,
  StartupPatch,
  SpellHit,
  SyntaxIssue,
  AssetReport,
  DirEntry,
  CatalogView,
  GitBranch,
  GitCommit,
  GitCommitFile,
  GitStatus,
  LogLine,
  RefHit,
  ResolveResult,
  Route,
  RenamePreview,
  SearchReport,
  TranslationEntry,
} from './types'

export const api = {
  initialProject: () => invoke<string | null>('initial_project'),
  initialLabel: () => invoke<string | null>('initial_label'),
  initialActions: () => invoke<string[]>('initial_actions'),
  initialPatch: () => invoke<StartupPatch | null>('initial_patch'),
  initialRevert: () => invoke<string | null>('initial_revert'),
  openProject: (path: string) => invoke<ProjectInfo>('open_project', { path }),
  editStatus: () => invoke<{ modified: string[]; decompiled: string[] }>('edit_status'),
  writeFile: (path: string, text: string) => invoke<EditImpact>('write_file', { path, text }),
  revertFile: (path: string) => invoke<EditImpact>('revert_file', { path }),
  checkSyntax: (text: string) => invoke<SyntaxIssue[]>('check_syntax', { text }),
  pylspStatus: () => invoke<{ installed: boolean; version: string; running: boolean; exe: string }>('pylsp_status'),
  pylspEnv: () => invoke<{
    scriptVersion: string | null
    pythonVersion: string | null
    sdkRoot: string | null
    python2: boolean
    compatDir: string
    unsupported: boolean
    reason: string
  }>('pylsp_env'),
  pylspInstall: (force: boolean) => invoke<void>('pylsp_install', { force }),
  pylspCancel: () => invoke<void>('pylsp_cancel'),
  pylspRemove: () => invoke<void>('pylsp_remove'),
  pylspStart: (settings: unknown) => invoke<void>('pylsp_start', { settings }),
  pylspSend: (message: unknown) => invoke<void>('pylsp_send', { message }),
  pylspStop: () => invoke<void>('pylsp_stop'),
  projectInfo: () => invoke<ProjectInfo | null>('get_project_info'),
  projectMap: () => invoke<ProjectMap>('get_project_map'),
  diagnostics: () => invoke<DiagReport>('get_diagnostics'),
  engineDump: (launcher: string | null) => invoke<ProjectInfo>('engine_dump', { launcher }),
  engineLint: (launcher: string | null) => invoke<ProjectInfo>('engine_lint', { launcher }),
  stageImages: (launcher: string | null) => invoke<ProjectInfo>('stage_images', { launcher }),
  liveImages: () => invoke<void>('live_images'),
  labelGraph: (name: string, detail = false) => invoke<LabelGraph>('get_label_graph', { name, detail }),
  labelLines: (name: string) => invoke<LabelLines>('get_label_lines', { name }),
  launchGame: (launcher: string | null) => invoke<LaunchReport>('launch_game', { launcher }),
  warpTo: (file: string, line: number, launcher: string | null) =>
    invoke<LaunchReport>('warp_to', { file, line, launcher }),
  liveStart: (launcher: string | null, file: string | null, line: number | null, watch: string[]) =>
    invoke<LiveReport>('live_start', { launcher, file, line, watch }),
  liveReplay: (file: string, line: number, launcher: string | null, watch: string[]) =>
    invoke<LiveReport>('live_replay', { file, line, launcher, watch }),
  liveJump: (file: string, line: number) => invoke<LiveReport>('live_jump', { file, line }),
  liveJumpLabel: (name: string) => invoke<LiveReport>('live_jump_label', { name }),
  liveReload: () => invoke<string>('live_reload'),
  liveSetBreaks: (points: string[]) => invoke<void>('live_set_breaks', { points }),
  liveResume: () => invoke<void>('live_resume'),
  liveStep: () => invoke<void>('live_step'),
  liveStop: () => invoke<void>('live_stop'),
  liveSetWatch: (names: string[]) => invoke<void>('live_set_watch', { names }),
  archiveFingerprints: () => invoke<ArchiveDigest[]>('archive_fingerprints'),
  layoutCacheGet: (key: string) => invoke<string | null>('layout_cache_get', { key }),
  layoutCachePut: (key: string, data: string) => invoke<void>('layout_cache_put', { key, data }),
  archiveList: (path: string) => invoke<ArchiveEntryInfo[]>('archive_list', { path }),
  archiveExtract: (archive: string, dest: string, names: string[] | null, allowInsideGame: boolean) =>
    invoke<number>('archive_extract', { archive, dest, names, allowInsideGame }),
  archiveBuild: (srcDir: string, outPath: string) => invoke<number>('archive_build', { srcDir, outPath }),
  archiveCancel: () => invoke<void>('archive_cancel'),
  patchBake: () => invoke<PatchReport>('patch_bake'),
  patchUndo: () => invoke<PatchReport>('patch_undo'),
  patchRemove: () => invoke<PatchReport>('patch_remove'),
  patchRebase: (rel: string) => invoke<RebaseResult>('patch_rebase', { rel }),
  modTogglesGet: () => invoke<ModToggles>('mod_toggles_get'),
  modTogglesSet: (toggles: ModToggles) => invoke<void>('mod_toggles_set', { toggles }),
  modExport: (dest: string, layout: 'rpa' | 'loose', includeToggles: boolean, notes: string) =>
    invoke<string>('mod_export', { dest, layout, includeToggles, notes }),
  saveList: () => invoke<SaveSlot[]>('save_list'),
  saveInspect: (path: string, deep: boolean) => invoke<SaveDetail>('save_inspect', { path, deep }),
  catalog: () => invoke<CatalogView>('get_catalog'),
  translations: (lang: string) => invoke<TranslationEntry[]>('get_translations', { lang }),
  labelRoutes: (name: string) => invoke<Route[]>('label_routes', { name }),
  searchProject: (query: string, dialogueOnly: boolean) =>
    invoke<SearchReport>('search_project', { query, dialogueOnly }),
  replaceText: (changes: { path: string; before: string; text: string }[]) =>
    invoke<EditImpact>('replace_text', { changes }),
  findReferences: (kind: string, name: string) => invoke<RefHit[]>('find_references', { kind, name }),
  resolveSymbol: (path: string, line: number, name: string, prefer: string | null) =>
    invoke<ResolveResult>('resolve_symbol', { path, line, name, prefer }),
  previewRename: (kind: string, oldName: string, newName: string) =>
    invoke<Omit<RenamePreview, 'kind' | 'oldName' | 'newName'>>('preview_rename', { kind, oldName, newName }),
  renameSymbol: (kind: string, oldName: string, newName: string) =>
    invoke<EditImpact>('rename_symbol', { kind, oldName, newName }),
  createScript: (path: string, text: string | null) => invoke<void>('create_script', { path, text }),
  renameScript: (path: string, newPath: string) => invoke<void>('rename_script', { path, newPath }),
  deleteScript: (path: string) => invoke<void>('delete_script', { path }),
  fsCreate: (path: string, dir: boolean) => invoke<string>('fs_create', { path, dir }),
  fsRename: (path: string, newPath: string) => invoke<void>('fs_rename', { path, newPath }),
  fsMove: (paths: string[], destDir: string) => invoke<string[]>('fs_move', { paths, destDir }),
  fsDelete: (path: string) => invoke<void>('fs_delete', { path }),
  fsReveal: (path: string) => invoke<void>('fs_reveal', { path }),
  updateTranslation: (path: string, line: number, text: string) =>
    invoke<EditImpact>('update_translation', { path, line, text }),
  assetReport: () => invoke<AssetReport>('asset_report'),
  listDir: (path: string) => invoke<DirEntry[]>('list_dir', { path }),
  readLogs: () => invoke<LogLine[]>('read_logs'),
  autoreloadEnabled: () => invoke<boolean>('autoreload_enabled'),
  setAutoreload: (enabled: boolean) => invoke<boolean>('set_autoreload', { enabled }),
  createProject: (parent: string, name: string, scene: string | null, sdk: string | null) =>
    invoke<string>('create_project', { parent, name, scene, sdk }),
  sdkList: () => invoke<{ folder: string; items: import('./sdk.svelte').SdkInfo[] }>('sdk_list'),
  sdkSetFolder: (folder: string) =>
    invoke<{ folder: string; items: import('./sdk.svelte').SdkInfo[] }>('sdk_set_folder', { folder }),
  sdkAdd: (path: string) => invoke<import('./sdk.svelte').SdkInfo>('sdk_add', { path }),
  sdkRemove: (path: string, deleteFiles: boolean) =>
    invoke<{ folder: string; items: import('./sdk.svelte').SdkInfo[] }>('sdk_remove', { path, deleteFiles }),
  sdkCatalog: () => invoke<string[]>('sdk_catalog'),
  sdkInstall: (version: string) => invoke<import('./sdk.svelte').SdkInfo>('sdk_install', { version }),
  sdkInstallWeb: (path: string) => invoke<import('./sdk.svelte').SdkInfo>('sdk_install_web', { path }),
  sdkCancel: () => invoke<void>('sdk_cancel'),
  buildPc: (sdk: string, dest: string | null, packages: string[]) =>
    invoke<string>('build_pc', { sdk, dest, packages }),
  buildWeb: (sdk: string, dest: string | null, launch: boolean) =>
    invoke<string>('build_web', { sdk, dest, launch }),
  buildCancel: () => invoke<void>('build_cancel'),
  revealPath: (path: string) => invoke<void>('reveal_path', { path }),
  applogRead: () => invoke<AppLogInput[]>('applog_read'),
  applogWrite: (level: string, source: string, message: string) =>
    invoke<void>('applog_write', { level, source, message }),
  applogClear: () => invoke<void>('applog_clear'),
  applogPath: () => invoke<string | null>('applog_path'),
  stageAt: (file: string, line: number, vars?: Record<string, string>) =>
    invoke<import('./types').StageEstimate>('stage_at', { file, line, vars: vars ?? {} }),
  liveShots: (on: boolean) => invoke<void>('live_shots', { on }),
  sceneEdit: (change: {
    op: string
    path?: string
    line?: number
    speaker?: string
    text?: string
    target?: string
    targetLine?: number
    name?: string
    color?: string
    source?: string
    newLabel?: boolean
    place?: 'before' | 'after' | 'into'
    dir?: -1 | 1
    cond?: string
    how?: 'jump' | 'call' | 'choice' | 'return'
    spec?: import('./types').StmtSpec
  }) => invoke<SceneReport>('scene_edit', { change }),
  sceneUndo: () => invoke<SceneReport>('scene_undo'),
  sceneRedo: () => invoke<SceneReport>('scene_redo'),
  sceneParse: (code: string) => invoke<import('./types').StmtSpec | null>('scene_parse', { code }),
  sceneHistory: () => invoke<import('./types').SceneHistoryState>('scene_history'),
  gitStatus: () => invoke<GitStatus>('git_status'),
  gitInit: () => invoke<string>('git_init'),
  gitWriteIgnore: () => invoke<void>('git_write_ignore'),
  gitIgnore: (path: string) => invoke<void>('git_ignore', { path }),
  gitBranches: () => invoke<GitBranch[]>('git_branches'),
  gitSwitch: (name: string) => invoke<void>('git_switch', { name }),
  gitCreateBranch: (name: string) => invoke<void>('git_create_branch', { name }),
  gitFetch: () => invoke<string>('git_fetch'),
  gitPull: () => invoke<string>('git_pull'),
  gitPush: () => invoke<string>('git_push'),
  gitAddRemote: (url: string) => invoke<void>('git_add_remote', { url }),
  gitStage: (paths: string[]) => invoke<void>('git_stage', { paths }),
  gitUnstage: (paths: string[]) => invoke<void>('git_unstage', { paths }),
  gitDiscard: (path: string) => invoke<void>('git_discard', { path }),
  gitShow: (rev: string, path: string) => invoke<string>('git_show', { rev, path }),
  gitStageText: (path: string, text: string) => invoke<void>('git_stage_text', { path, text }),
  gitLog: (path: string | null, limit: number) => invoke<GitCommit[]>('git_log', { path, limit }),
  gitCommitFiles: (rev: string) => invoke<GitCommitFile[]>('git_commit_files', { rev }),
  gitCommit: (message: string, amend = false) => invoke<string>('git_commit', { message, amend }),
  gitUndoCommit: () => invoke<string>('git_undo_commit'),
  spellCheck: (text: string) => invoke<SpellHit[]>('spell_check', { text }),
  spellSuggest: (word: string) => invoke<string[]>('spell_suggest', { word }),
  spellAddWord: (word: string) => invoke<void>('spell_add_word', { word }),
  spellWords: () => invoke<string[]>('spell_words'),
  spellRemoveWord: (word: string) => invoke<void>('spell_remove_word', { word }),
  diagnosticBundle: () => invoke<string>('diagnostic_bundle'),
  settingsExport: (path: string, text: string) => invoke<void>('settings_export', { path, text }),
  settingsImport: (path: string) => invoke<string>('settings_import', { path }),
}

export async function readAsset(path: string): Promise<ArrayBuffer> {
  return invoke<ArrayBuffer>('read_asset', { path })
}

/** Project-relative bytes for a text, image, audio, or video preview. */
export async function readPreview(path: string): Promise<ArrayBuffer> {
  return invoke<ArrayBuffer>('read_preview', { path })
}

const decoder = new TextDecoder('utf-8')

/** File contents; the backend sends raw bytes to avoid JSON-encoding megabytes of text. */
export async function readFileText(path: string): Promise<string> {
  const bytes = await invoke<ArrayBuffer>('read_file', { path })
  const text = decoder.decode(bytes)
  return text.charCodeAt(0) === 0xfeff ? text.slice(1) : text
}

export function onLiveState(cb: (s: LiveState) => void): Promise<UnlistenFn> {
  return listen<LiveState>('live-state', (e) => cb(e.payload))
}

export function onLiveShot(cb: (s: { seq: number; file: string; line: number }) => void): Promise<UnlistenFn> {
  return listen<{ seq: number; file: string; line: number }>('live-shot', (e) => cb(e.payload))
}

export function onStageImages(cb: (info: ProjectInfo) => void): Promise<UnlistenFn> {
  return listen<ProjectInfo>('stage-images', (e) => cb(e.payload))
}

export async function readLiveShot(): Promise<ArrayBuffer> {
  return invoke<ArrayBuffer>('live_shot')
}

export function onProjectChanged(cb: (p: ChangedPayload) => void): Promise<UnlistenFn> {
  return listen<ChangedPayload>('project:changed', (e) => cb(e.payload))
}

export function onRpaProgress(cb: (p: RpaProgress) => void): Promise<UnlistenFn> {
  return listen<RpaProgress>('rpa:progress', (e) => cb(e.payload))
}

export function onSdkProgress(
  cb: (p: { version: string; phase: string; done: number; total: number; label: string }) => void,
): Promise<UnlistenFn> {
  return listen<{ version: string; phase: string; done: number; total: number; label: string }>(
    'sdk:progress',
    (e) => cb(e.payload),
  )
}

export function onBuildLine(cb: (line: { stream: string; text: string }) => void): Promise<UnlistenFn> {
  return listen<{ stream: string; text: string }>('build:line', (e) => cb(e.payload))
}

export function onApplogEntry(cb: (entry: AppLogInput) => void): Promise<UnlistenFn> {
  return listen<AppLogInput>('applog:entry', (e) => cb(e.payload))
}

export async function readArchiveEntry(archive: string, name: string): Promise<ArrayBuffer> {
  return invoke<ArrayBuffer>('archive_read_entry', { archive, name })
}

export async function pickProjectFolder(): Promise<string | null> {
  const result = await open({ directory: true, multiple: false, title: 'Open Ren\'Py project' })
  return typeof result === 'string' ? result : null
}

export async function pickFolder(title: string): Promise<string | null> {
  const result = await open({ directory: true, multiple: false, title })
  return typeof result === 'string' ? result : null
}

export async function pickZipPath(title: string, name: string): Promise<string | null> {
  const result = await save({ title, defaultPath: name, filters: [{ name: 'Zip archive', extensions: ['zip'] }] })
  return typeof result === 'string' ? result : null
}

export async function pickSavePath(title: string, name: string): Promise<string | null> {
  const result = await save({ title, defaultPath: name, filters: [{ name: 'Ren\'Py archive', extensions: ['rpa'] }] })
  return typeof result === 'string' ? result : null
}

export async function pickImage(): Promise<string | null> {
  const result = await open({
    multiple: false,
    title: 'Choose an image',
    filters: [{ name: 'Image', extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif'] }],
  })
  return typeof result === 'string' ? result : null
}

export async function pickLauncher(): Promise<string | null> {
  const result = await open({
    multiple: false,
    title: 'Choose the game or renpy.exe launcher',
    filters: [{ name: 'Executable', extensions: ['exe', 'sh', 'bat'] }],
  })
  return typeof result === 'string' ? result : null
}

export async function pickSdk(): Promise<string | null> {
  const result = await open({
    multiple: false,
    title: 'Choose renpy.exe in a Ren\'Py SDK',
    filters: [{ name: 'Ren\'Py launcher', extensions: ['exe', 'sh'] }],
  })
  return typeof result === 'string' ? result : null
}

const SETTINGS_FILTER = [{ name: 'Settings', extensions: ['json'] }]

export async function pickSettingsSave(): Promise<string | null> {
  const result = await save({ title: 'Export settings', defaultPath: 'vn-ide-settings.json', filters: SETTINGS_FILTER })
  return typeof result === 'string' ? result : null
}

export async function pickSettingsOpen(): Promise<string | null> {
  const result = await open({ multiple: false, title: 'Import settings', filters: SETTINGS_FILTER })
  return typeof result === 'string' ? result : null
}

export function errorText(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  try {
    return JSON.stringify(e)
  } catch {
    return String(e)
  }
}

const QUIET_COMMANDS = new Set(['applog_write', 'applog_read', 'applog_clear', 'applog_path'])

/** A project folder with no repository is a normal state, not an application fault. */
function quietFailure(command: string, err: unknown): boolean {
  if (QUIET_COMMANDS.has(command)) return true
  return errorText(err).toLowerCase().includes('not a git repository')
}

function invoke<T>(command: string, args?: InvokeArgs): Promise<T> {
  return rawInvoke<T>(command, args).catch((err: unknown) => {
    if (!quietFailure(command, err)) {
      logUi('error', 'ui', `${command} failed: ${errorText(err)}`)
    }
    throw err
  })
}
