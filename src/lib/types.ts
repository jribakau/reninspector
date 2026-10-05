// Mirrors the serde(camelCase) output of renpy-core.

export interface FileInfo {
  path: string
  lines: number
  bytes: number
  opaque: number
  labels: number
  issues: number
  /** `loose`, `archived`, `override` or `compiled`. */
  origin: string
  archive: string | null
  /** False when a compiled script could not be fully recovered. */
  editable: boolean
  /** True when the text was recovered from a `.rpyc`. */
  decompiled: boolean
  /** Why a decompiled file is read-only. Empty when it is editable. */
  reasons: string[]
}

export interface FormatCount {
  version: string
  count: number
}

export interface GameLayout {
  looseScripts: number
  overrideScripts: number
  archivedScripts: number
  compiledScripts: number
  compiledOnly: number
  archives: number
  nestedArchives: number
  formats: FormatCount[]
}

export interface ArchiveDigest {
  path: string
  bytes: number
  modified: string | null
  sha256: string | null
}

export interface GameInfo {
  engineVersion: string | null
  scriptVersion: string | null
  name: string | null
  version: string | null
  buildName: string | null
  saveDirectory: string | null
  /** Oldest modification time of archive and compiled files. A heuristic, not a build date. */
  filesOldest: string | null
  filesNewest: string | null
  layout: GameLayout
  archives: ArchiveDigest[]
  notes: string[]
}

export interface ArchiveInfo {
  path: string
  version: string
  /** Non-official header. Readable, but not rewritten. */
  readOnlyFormat: boolean
  entries: number
  scripts: number
  compiledOnly: number
  otherBytes: number
  isPatch: boolean
  nested: boolean
  overrides: number
  stale: string[]
  error: string | null
}

export interface ArchiveEntryInfo {
  name: string
  len: number
  kind: string
}

export interface PatchReport {
  patch: string
  files: number
  action: string
}

export interface ModToggles {
  console: boolean
  developer: boolean
  quickSaveKeys: boolean
  skipUnseen: boolean
  rollback: boolean
}

export interface RebaseResult {
  base: string
  upstream: string
  mine: string
  merged: string
  conflicts: number
  baseMissing: boolean
}

export interface SaveSlot {
  path: string
  name: string
  extra: string
  version: string
  modified: string | null
  persistent: boolean
}

export interface SaveNode {
  name: string
  kind: string
  repr: string
  children: SaveNode[]
}

export interface SaveDetail {
  slot: SaveSlot
  json: string
  screenshotBase64: string | null
  tree: SaveNode | null
  note: string | null
}

export interface RpaProgress {
  done: number
  total: number
  label: string
}

export interface Stats {
  files: number
  lines: number
  labels: number
  menus: number
  says: number
  edges: number
  opaque: number
  dynamicJumps: number
  missingTargets: number
  unreachable: number
  duplicates: number
  compiledLabels: number
  screens: number
}

export interface ProjectInfo {
  root: string
  gameDir: string
  name: string
  engineVersion: string | null
  launcher: string | null
  bundledEngine: boolean
  developer: string | null
  scriptVersion: string | null
  hasArchives: boolean
  compiledOnly: string[]
  archives: ArchiveInfo[]
  files: FileInfo[]
  stats: Stats
  parseMs: number
  engine: EngineSummary | null
  stageImages: StageImagesSummary | null
  lintCount: number | null
  game: GameInfo
}

export interface StageImagesSummary {
  count: number
  stale: boolean
  notes: string[]
  durationMs: number
}

export interface EngineSummary {
  labels: number
  screens: number
  defines: number
  labelsAvailable: boolean
  notes: string[]
  durationMs: number
  fromCache: boolean
  stale: boolean
}

export type MapNodeKind = 'label' | 'menu' | 'missing' | 'compiled' | 'screen'

export interface MapNode {
  id: string
  kind: MapNodeKind
  file: number
  line: number
  endLine: number
  stmts: number
  says: number
  menus: number
  choices: number
  inDegree: number
  outDegree: number
  reachable: boolean
  root: boolean
  indirect: boolean
  duplicate: boolean
  returns: boolean
  endsScript: boolean
  dynamicOut: number
}

export type MapEdgeKind = 'jump' | 'choice' | 'fall' | 'call' | 'screen' | 'action'

export interface MapEdge {
  from: string
  to: string
  kind: MapEdgeKind
  count: number
}

export interface ProjectMap {
  nodes: MapNode[]
  edges: MapEdge[]
}

export type Severity = 'error' | 'warning' | 'info'

export interface Diagnostic {
  severity: Severity
  code: string
  message: string
  path: string
  line: number
  label: string | null
}

export interface DiagReport {
  items: Diagnostic[]
  errors: number
  warnings: number
  infos: number
  truncated: number
}

export type GKind = 'dialogue' | 'menu' | 'choice' | 'cond' | 'jump' | 'fall' | 'call' | 'screen' | 'return' | 'end'
export type EKind = 'next' | 'choice' | 'branch'

export interface GNode {
  id: number
  kind: GKind
  line: number
  endLine: number
  title: string
  target: string | null
  /** For screen nodes: labels the screen can jump to. */
  targets: string[]
  dynamic: boolean
  says: number
  stmts: number
  variants: number
  conds: number
  opaque: number
  python: number
  choices: number
  speakers: string[]
  preview: string[]
  /** Spoken line or choice caption. Empty on a folded overview run. */
  body: string
  /** `body` was cut. Saving it would drop the rest of the line. */
  clipped: boolean
  /** Line of a choice's jump, when that jump is the whole choice. */
  targetLine: number
  /** Detail mode, beat cards: the staging statements on the card. */
  beats: BeatLine[]
}

export interface BeatLine {
  line: number
  endLine: number
  /** `show`, `scene`, `play`, `$`, ... */
  cmd: string
  text: string
  /** The flow editor can rewrite this line. */
  editable: boolean
}

/** A one-line statement the flow editor writes. */
export type StmtSpec =
  | { kind: 'say'; speaker: string; text: string }
  | { kind: 'scene'; image: string; at?: string; with?: string }
  | { kind: 'show'; image: string; at?: string; with?: string }
  | { kind: 'hide'; image: string; with?: string }
  | { kind: 'with'; transition: string }
  | { kind: 'play'; channel: string; file: string; fadein?: string; looped?: boolean }
  | { kind: 'stop'; channel: string; fadeout?: string }
  | { kind: 'pause'; secs?: string }
  | { kind: 'jump'; target: string }
  | { kind: 'call'; target: string }
  | { kind: 'return' }

export interface GEdge {
  from: number
  to: number
  kind: EKind
  label: string | null
  cond: string | null
}

export interface ScriptLine {
  kind: string
  speaker: string
  text: string
  /** Jump or call target. On a choice, the scene that choice jumps to. */
  target: string
  line: number
  endLine: number
  /** Line of `target` when it is a jump under a choice. */
  targetLine: number
  depth: number
  /** The text was shortened. Writing it back would cut the line. */
  clipped: boolean
}

export interface SceneReport {
  path: string
  impact: EditImpact
  /** 1-based line that was inserted, moved, or edited. 0 when there is none. */
  focusLine: number
  canUndo: boolean
  canRedo: boolean
}

export interface SceneHistoryState {
  canUndo: boolean
  canRedo: boolean
}

export interface LabelLines {
  name: string
  lines: ScriptLine[]
  truncated: boolean
}

export interface LabelGraph {
  name: string
  kind: string
  line: number
  endLine: number
  root: number
  nodes: GNode[]
  edges: GEdge[]
}

export interface LaunchReport {
  command: string
  warp: string | null
  notes: string[]
}

/** Where the running game says it is. `vars` values are JSON scalars. */
export interface LiveState {
  running: boolean
  file: string
  line: number
  label: string
  showing: string[]
  speaker: string
  vars: Record<string, string | number | boolean | null>
  canWarp: boolean
  canReload: boolean
  /** `pending`, `running`, `done`, `diverged`, `stalled`, or empty. */
  replay: string
  replayReason: string
  note: string
  /** Lines from the last warp: return target, a cut path, names treated as false. */
  warpNotes: string[]
}

export interface LiveReport {
  notes: string[]
  warp: string | null
  label: string | null
  assumptions?: string[]
}

export interface ChangedPayload {
  paths: string[]
}

/** What a save or revert changed in the analysis. */
export interface EditImpact {
  added: Diagnostic[]
  removed: Diagnostic[]
  labelsAdded: string[]
  labelsRemoved: string[]
  becameUnreachable: string[]
  becameReachable: string[]
}

export interface SyntaxIssue {
  line: number
  message: string
}

export interface SpellHit {
  line: number
  from: number
  to: number
  word: string
}

export interface StartupPatch {
  path: string
  find: string
  replace: string
}

export interface Symbol {
  kind: string
  name: string
  path: string
  line: number
  detail: string
}

export interface VariableInfo {
  name: string
  keyword: string
  path: string
  line: number
  value: string
  uses: number
  persistent: boolean
}

export interface LanguageStat {
  lang: string
  total: number
  translated: number
}

export interface SpeakerStat {
  name: string
  lines: number
  words: number
}

export interface DialogueStats {
  lines: number
  words: number
  minutes: number
  speakers: SpeakerStat[]
}

export interface CatalogView {
  symbols: Symbol[]
  variables: VariableInfo[]
  languages: LanguageStat[]
  dialogue: DialogueStats
  endings: string[]
  deadEnds: string[]
}

export interface TranslationEntry {
  path: string
  line: number
  lang: string
  kind: string
  source: string
  translated: string
}

export interface SearchHit {
  path: string
  line: number
  text: string
  kind: string
  speaker: string
}

export interface SearchReport {
  hits: SearchHit[]
  truncated: boolean
}

export interface RenameEdit {
  path: string
  line: number
  before: string
  after: string
}

export interface RenamePreview {
  kind: string
  oldName: string
  newName: string
  hits: RenameEdit[]
  fileCount: number
  truncated: boolean
}

export interface RefHit {
  path: string
  line: number
  text: string
}

export interface ResolveResult {
  hit: boolean
  symbol: Symbol | null
}

export interface Route {
  steps: string[]
}

export interface DirEntry {
  name: string
  path: string
  dir: boolean
}

export interface AssetFile {
  path: string
  kind: string
  bytes: number
  used: boolean
}

export interface MissingAsset {
  path: string
  line: number
  file: string
  what: string
}

export interface AssetReport {
  files: AssetFile[]
  filesTotal: number
  missing: MissingAsset[]
  missingTotal: number
  unused: number
}

export interface LogLine {
  source: string
  text: string
  path: string | null
  line: number | null
}

export interface GitChange {
  path: string
  status: string
  staged: boolean
}

export interface GitStatus {
  branch: string
  upstream: string | null
  ahead: number
  behind: number
  hasRemote: boolean
  hasIgnore: boolean
  /** Staged paths outside the project folder. */
  outsideStaged: string[]
  /** True when the current commit is already on the upstream branch. */
  headPushed: boolean
  /** Remote a first push would use, or null when several remotes make it ambiguous. */
  pushRemote: string | null
  changes: GitChange[]
}

export interface GitBranch {
  name: string
  current: boolean
  remote: boolean
  upstream: string
}

export interface GitCommit {
  hash: string
  author: string
  date: string
  subject: string
}

export interface GitCommitFile {
  status: string
  path: string
}

export type StagePicture =
  | { kind: 'file'; path: string }
  | { kind: 'color'; hex: string }
  | { kind: 'layers'; w: number; h: number; layers: { path: string; x: number; y: number }[] }
  | { kind: 'frames'; frames: { picture: StagePicture; seconds: number }[]; repeat: boolean }
  | { kind: 'unknown' }

export interface StageSprite {
  tag: string
  name: string
  layer: string
  picture: StagePicture
  xalign: number | null
  yalign: number | null
  xpos: number | null
  ypos: number | null
  xanchor: number | null
  yanchor: number | null
  zoom: number | null
  sizeW: number | null
  sizeH: number | null
  fit: string | null
  placeFit: string | null
  flip: boolean
  shownFile: string
  shownLine: number
  defineFile: string | null
  defineLine: number | null
}

export interface StageSay {
  who: string | null
  whoColor: string | null
  what: string
}

export interface StageChoice {
  text: string
  conditional: boolean
  file: string
  line: number
}

export interface StageGui {
  textbox: string | null
  namebox: string | null
  choice: string | null
  textboxHeight: number
  textboxYalign: number
  nameXpos: number
  nameYpos: number
  nameXalign: number
  dialogueXpos: number
  dialogueYpos: number
  dialogueWidth: number
  textSize: number
  nameTextSize: number
  textColor: string
  accentColor: string
  choiceWidth: number
  choiceTextSize: number
  choiceSpacing: number
  choiceTextColor: string
  choiceYpos: number
}

export interface StageVarUse {
  file: string
  line: number
  cond: string
}

export interface StageVar {
  name: string
  /** Literal the preview used. Empty when `origin` is `unset`. */
  value: string
  /** `pinned`, `script` or `unset`. */
  origin: string
  uses: StageVarUse[]
  /** Uses of this name that were not listed. */
  more: number
}

export interface StageEstimate {
  width: number
  height: number
  sprites: StageSprite[]
  say: StageSay | null
  choices: StageChoice[]
  gui: StageGui
  via: string
  decisions: number
  assumptions: string[]
  notes: string[]
  vars: StageVar[]
  /** Variables beyond `vars`. */
  varsMore: number
}
