/**
 * User settings. One typed store, saved under a single localStorage key as the
 * values that differ from their defaults. This file imports nothing from the
 * rest of the app so every module can read it.
 */

export type ThemeChoice = 'dark' | 'light' | 'system'
export type AutosaveMode = 'off' | 'delay' | 'blur'

export interface Settings {
  theme: ThemeChoice
  fontFamily: string
  fontSize: number
  lineHeight: number
  indentWidth: number
  wrap: boolean
  lineNumbers: boolean
  activeLine: boolean
  bracketMatching: boolean
  closeBrackets: boolean
  autocomplete: boolean
  keymap: 'default' | 'vim' | 'emacs'
  colorSwatches: boolean
  inlayHints: boolean
  formatOnSave: boolean
  preferredQuote: 'keep' | 'double' | 'single'
  foldGutter: boolean
  spell: boolean
  autosave: AutosaveMode
  autosaveDelay: number
  restoreSession: boolean
  maxRecent: number
  maxTabs: number
  previewTabs: boolean
  followGame: boolean
  stageAnimate: boolean
  followCaret: boolean
  diffSplit: boolean
  gitTree: boolean
}

export type SettingKey = keyof Settings
export type CategoryId = 'appearance' | 'editor' | 'files' | 'live' | 'git' | 'layout' | 'project'

export const CATEGORIES: { id: CategoryId; label: string }[] = [
  { id: 'appearance', label: 'Appearance' },
  { id: 'editor', label: 'Editor' },
  { id: 'files', label: 'Files and saving' },
  { id: 'live', label: 'Live and flow' },
  { id: 'git', label: 'Source control' },
  { id: 'layout', label: 'Layout' },
  { id: 'project', label: 'This project' },
]

export interface SettingDef {
  key: SettingKey
  category: CategoryId
  label: string
  description: string
  type: 'bool' | 'number' | 'enum' | 'text'
  options?: { value: string | number; label: string }[]
  min?: number
  max?: number
  step?: number
  placeholder?: string
  unit?: string
}

export const SETTINGS_KEY = 'vnide.settings'

function reducedMotion(): boolean {
  return typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches
}

export const DEFAULTS: Settings = {
  theme: 'dark',
  fontFamily: '',
  fontSize: 13,
  lineHeight: 1.55,
  indentWidth: 4,
  wrap: false,
  lineNumbers: true,
  activeLine: true,
  bracketMatching: true,
  closeBrackets: true,
  autocomplete: true,
  keymap: 'default',
  colorSwatches: true,
  inlayHints: true,
  formatOnSave: false,
  preferredQuote: 'keep',
  foldGutter: true,
  spell: true,
  autosave: 'off',
  autosaveDelay: 2000,
  restoreSession: true,
  maxRecent: 8,
  maxTabs: 16,
  previewTabs: true,
  followGame: true,
  stageAnimate: !reducedMotion(),
  followCaret: true,
  diffSplit: false,
  gitTree: false,
}

export const SETTINGS: SettingDef[] = [
  {
    key: 'theme',
    category: 'appearance',
    label: 'Color theme',
    description: 'Dark, light, or follow the operating system.',
    type: 'enum',
    options: [
      { value: 'dark', label: 'Dark' },
      { value: 'light', label: 'Light' },
      { value: 'system', label: 'Match system' },
    ],
  },
  {
    key: 'fontFamily',
    category: 'appearance',
    label: 'Editor font family',
    description: 'A CSS font list, such as "Fira Code, Consolas". Empty uses the built-in monospace stack.',
    type: 'text',
    placeholder: 'Cascadia Code, JetBrains Mono, Consolas',
  },
  {
    key: 'fontSize',
    category: 'appearance',
    label: 'Editor font size',
    description: 'Size of the script editor text.',
    type: 'number',
    min: 11,
    max: 22,
    step: 1,
    unit: 'px',
  },
  {
    key: 'lineHeight',
    category: 'appearance',
    label: 'Editor line height',
    description: 'Space between lines, as a multiple of the font size.',
    type: 'number',
    min: 1.2,
    max: 2.2,
    step: 0.05,
  },
  {
    key: 'indentWidth',
    category: 'editor',
    label: 'Indent width',
    description: 'Spaces inserted by Tab and used when the editor indents a new line. Ren\'Py does not accept tab characters.',
    type: 'enum',
    options: [
      { value: 2, label: '2 spaces' },
      { value: 4, label: '4 spaces' },
      { value: 8, label: '8 spaces' },
    ],
  },
  {
    key: 'wrap',
    category: 'editor',
    label: 'Word wrap',
    description: 'Wrap long lines instead of scrolling sideways. Alt+Z toggles it.',
    type: 'bool',
  },
  {
    key: 'lineNumbers',
    category: 'editor',
    label: 'Line numbers',
    description: 'Show line numbers in the gutter.',
    type: 'bool',
  },
  {
    key: 'foldGutter',
    category: 'editor',
    label: 'Folding markers',
    description: 'Show the arrows that collapse labels and blocks.',
    type: 'bool',
  },
  {
    key: 'activeLine',
    category: 'editor',
    label: 'Highlight current line',
    description: 'Tint the line the caret is on.',
    type: 'bool',
  },
  {
    key: 'bracketMatching',
    category: 'editor',
    label: 'Bracket matching',
    description: 'Highlight the bracket that pairs with the one next to the caret.',
    type: 'bool',
  },
  {
    key: 'closeBrackets',
    category: 'editor',
    label: 'Auto-close brackets and quotes',
    description: 'Type an opening bracket or quote and the closing one is added.',
    type: 'bool',
  },
  {
    key: 'autocomplete',
    category: 'editor',
    label: 'Autocomplete',
    description: 'Suggest labels, characters, images, and keywords while typing.',
    type: 'bool',
  },
  {
    key: 'keymap',
    category: 'editor',
    label: 'Keymap',
    description: 'Vim and Emacs replace the usual editing keys. In Vim, :w still saves. Ctrl+S and the menus keep working.',
    type: 'enum',
    options: [
      { value: 'default', label: 'Default' },
      { value: 'vim', label: 'Vim' },
      { value: 'emacs', label: 'Emacs' },
    ],
  },
  {
    key: 'colorSwatches',
    category: 'editor',
    label: 'Colour swatches',
    description: 'Show a colour chip after "#rrggbb" inside strings. Click the chip to change the colour.',
    type: 'bool',
  },
  {
    key: 'inlayHints',
    category: 'editor',
    label: 'Inlay hints',
    description: 'Show a character\'s display name, where a jump or call goes, and the word count of a label.',
    type: 'bool',
  },
  {
    key: 'formatOnSave',
    category: 'editor',
    label: 'Format on save',
    description: 'Before saving a script, turn tabs into spaces and remove trailing whitespace. Ren\'Py does not accept tab characters.',
    type: 'bool',
  },
  {
    key: 'preferredQuote',
    category: 'editor',
    label: 'Quote style',
    description: 'When formatting, rewrite simple strings that have no escapes or interpolation. Strings that need the other quote are left alone.',
    type: 'enum',
    options: [
      { value: 'keep', label: 'Leave quotes as written' },
      { value: 'double', label: 'Double quotes' },
      { value: 'single', label: 'Single quotes' },
    ],
  },
  {
    key: 'spell',
    category: 'editor',
    label: 'Spell check',
    description: 'Underline words in dialogue and menu choices that are not in the English dictionary.',
    type: 'bool',
  },
  {
    key: 'autosave',
    category: 'files',
    label: 'Autosave',
    description: 'Save changed scripts automatically, after a delay or when the window loses focus.',
    type: 'enum',
    options: [
      { value: 'off', label: 'Off' },
      { value: 'delay', label: 'After a delay' },
      { value: 'blur', label: 'When the window loses focus' },
    ],
  },
  {
    key: 'autosaveDelay',
    category: 'files',
    label: 'Autosave delay',
    description: 'How long after a change to save, when Autosave is set to After a delay.',
    type: 'number',
    min: 500,
    max: 60000,
    step: 500,
    unit: 'ms',
  },
  {
    key: 'restoreSession',
    category: 'files',
    label: 'Restore open tabs',
    description: 'Reopen the tabs, panels, and caret position a project had when it was last closed.',
    type: 'bool',
  },
  {
    key: 'maxRecent',
    category: 'files',
    label: 'Recent projects',
    description: 'How many projects the Open Recent list remembers.',
    type: 'number',
    min: 1,
    max: 30,
    step: 1,
  },
  {
    key: 'maxTabs',
    category: 'files',
    label: 'Maximum open tabs',
    description: 'When this many tabs are open, opening another closes the preview tab, then the least recently used unpinned one.',
    type: 'number',
    min: 4,
    max: 64,
    step: 1,
  },
  {
    key: 'previewTabs',
    category: 'files',
    label: 'Preview tabs',
    description: 'Reuse one italic tab when you click around. Double-click the tab, or edit the file, to keep it.',
    type: 'bool',
  },
  {
    key: 'followGame',
    category: 'live',
    label: 'Follow the running game',
    description: 'Move the editor caret to the line the live game is on.',
    type: 'bool',
  },
  {
    key: 'stageAnimate',
    category: 'live',
    label: 'Animate the stage preview',
    description: 'Play frame animations in the stage pane. Off by default when the system asks for reduced motion.',
    type: 'bool',
  },
  {
    key: 'followCaret',
    category: 'live',
    label: 'Flow follows the caret',
    description: 'Keep the flow card for the line under the caret in view.',
    type: 'bool',
  },
  {
    key: 'diffSplit',
    category: 'git',
    label: 'Side-by-side diffs',
    description: 'Show changes in two columns instead of one inline view.',
    type: 'bool',
  },
  {
    key: 'gitTree',
    category: 'git',
    label: 'Show changes as a tree',
    description: 'Group changed files by folder in the Git panel.',
    type: 'bool',
  },
]

const DEFS = new Map<SettingKey, SettingDef>(SETTINGS.map((def) => [def.key, def]))

/** The value if it is valid for this setting, clamped where that makes sense. Otherwise undefined. */
export function coerce(key: SettingKey, value: unknown): Settings[SettingKey] | undefined {
  const def = DEFS.get(key)
  if (!def) return undefined
  switch (def.type) {
    case 'bool':
      return typeof value === 'boolean' ? value : undefined
    case 'number': {
      if (typeof value !== 'number' || !Number.isFinite(value)) return undefined
      let n = Math.min(def.max ?? Infinity, Math.max(def.min ?? -Infinity, value))
      const step = def.step ?? 0
      if (step > 0) {
        const base = def.min ?? 0
        n = base + Math.round((n - base) / step) * step
        n = Math.min(def.max ?? Infinity, Math.max(def.min ?? -Infinity, n))
        n = Number(n.toFixed(4))
      }
      return n
    }
    case 'enum':
      return def.options?.some((o) => o.value === value) ? (value as Settings[SettingKey]) : undefined
    case 'text':
      return typeof value === 'string' ? value.trim().slice(0, 200) : undefined
  }
}

export const settings: Settings = $state({ ...DEFAULTS })

/** Whether the colors in use are the light ones, once "system" has been resolved. */
export const resolved = $state({ light: false })

/** Bumped when saved panel sizes are cleared so panels read their defaults again. */
export const layout = $state({ seq: 0 })

/** Whether the Settings dialog is open, and what it is showing. */
export const settingsUi = $state({ open: false, category: 'appearance' as CategoryId, query: '' })

export function openSettings(category?: CategoryId, query = '') {
  if (category) settingsUi.category = category
  settingsUi.query = query
  settingsUi.open = true
}

export function closeSettings() {
  settingsUi.open = false
}

function systemLight(): boolean {
  return typeof matchMedia === 'function' && matchMedia('(prefers-color-scheme: light)').matches
}

export function applyDocument() {
  resolved.light = settings.theme === 'light' || (settings.theme === 'system' && systemLight())
  if (typeof document === 'undefined') return
  const root = document.documentElement
  root.classList.toggle('light', resolved.light)
  root.style.setProperty('--code-size', `${settings.fontSize}px`)
  root.style.setProperty('--code-line', String(settings.lineHeight))
  if (settings.fontFamily) root.style.setProperty('--code-font', `${settings.fontFamily}, var(--mono)`)
  else root.style.removeProperty('--code-font')
}

function overrides(): Partial<Settings> {
  const out: Record<string, unknown> = {}
  for (const def of SETTINGS) {
    if (settings[def.key] !== DEFAULTS[def.key]) out[def.key] = settings[def.key]
  }
  return out as Partial<Settings>
}

function persist() {
  try {
    const diff = overrides()
    if (Object.keys(diff).length) localStorage.setItem(SETTINGS_KEY, JSON.stringify(diff))
    else localStorage.removeItem(SETTINGS_KEY)
  } catch {
    // Storage can be full or blocked. The values still apply for this session.
  }
}

function assign<K extends SettingKey>(key: K, value: Settings[K]) {
  settings[key] = value
}

/** Settings the IDE kept in separate keys before there was a Settings dialog. */
function legacyValues(): Partial<Settings> {
  const out: Record<string, unknown> = {}
  const get = (key: string) => localStorage.getItem(key)
  const theme = get('vnide.theme')
  if (theme === 'light') out.theme = 'light'
  const font = Number(get('vnide.font'))
  if (font) out.fontSize = font
  if (get('vnide.wrap') === '1') out.wrap = true
  if (get('vnide.spell') === '0') out.spell = false
  if (get('vnide.follow') === '0') out.followGame = false
  const animate = get('vnide.stageAnimate')
  if (animate === '1') out.stageAnimate = true
  else if (animate === '0') out.stageAnimate = false
  if (get('vnide.followCaret') === '0') out.followCaret = false
  if (get('vnide.diff.split') === '1') out.diffSplit = true
  if (get('vnide.scm.tree') === '1') out.gitTree = true
  return out as Partial<Settings>
}

const LEGACY_KEYS = [
  'vnide.theme',
  'vnide.font',
  'vnide.wrap',
  'vnide.spell',
  'vnide.follow',
  'vnide.stageAnimate',
  'vnide.followCaret',
  'vnide.diff.split',
  'vnide.scm.tree',
]

/** Reads saved settings, moving the old separate keys over the first time. */
export function loadSettings() {
  let saved: Record<string, unknown> = {}
  let migrate = false
  try {
    const raw = localStorage.getItem(SETTINGS_KEY)
    if (raw) {
      const parsed = JSON.parse(raw) as unknown
      if (parsed && typeof parsed === 'object') saved = parsed as Record<string, unknown>
    } else {
      saved = legacyValues()
      migrate = true
    }
  } catch {
    saved = {}
  }
  for (const def of SETTINGS) assign(def.key, DEFAULTS[def.key] as never)
  for (const [key, value] of Object.entries(saved)) {
    const checked = coerce(key as SettingKey, value)
    if (checked !== undefined) assign(key as SettingKey, checked as never)
  }
  if (migrate) {
    persist()
    for (const key of LEGACY_KEYS) localStorage.removeItem(key)
  }
  applyDocument()
}

export function setSetting<K extends SettingKey>(key: K, value: Settings[K]): boolean {
  const checked = coerce(key, value)
  if (checked === undefined) return false
  if (settings[key] === checked) return true
  assign(key, checked as Settings[K])
  persist()
  if (key === 'theme' || key === 'fontFamily' || key === 'fontSize' || key === 'lineHeight') applyDocument()
  return true
}

export function resetSetting(key: SettingKey) {
  setSetting(key, DEFAULTS[key] as never)
}

export function resetAll() {
  for (const def of SETTINGS) assign(def.key, DEFAULTS[def.key] as never)
  persist()
  applyDocument()
}

export function isModified(key: SettingKey): boolean {
  return settings[key] !== DEFAULTS[key]
}

/** Every setting with its current value, as pretty JSON for a file. */
export function exportSettings(): string {
  const all: Record<string, unknown> = {}
  for (const def of SETTINGS) all[def.key] = settings[def.key]
  return JSON.stringify({ app: 'vn-ide', settings: all }, null, 2)
}

/** Applies a settings file. Unknown keys and invalid values are skipped. Throws when the text is not settings JSON. */
export function importSettings(text: string): { applied: number; skipped: number } {
  let parsed: unknown
  try {
    parsed = JSON.parse(text)
  } catch {
    throw new Error('That file is not valid JSON.')
  }
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) throw new Error('That file does not hold settings.')
  const body = (parsed as { settings?: unknown }).settings
  const source = body && typeof body === 'object' && !Array.isArray(body) ? body : parsed
  let applied = 0
  let skipped = 0
  for (const [key, value] of Object.entries(source as Record<string, unknown>)) {
    if (key === 'app') continue
    if (!DEFS.has(key as SettingKey)) {
      skipped += 1
      continue
    }
    const checked = coerce(key as SettingKey, value)
    if (checked === undefined) {
      skipped += 1
      continue
    }
    assign(key as SettingKey, checked as never)
    applied += 1
  }
  persist()
  applyDocument()
  return { applied, skipped }
}

const SIZE_PREFIXES = ['vnide.w.', 'vnide.h.', 'vnide.ex.']

/** Forgets dragged panel sizes and collapsed explorer sections. */
export function resetLayout() {
  const doomed: string[] = []
  for (let i = 0; i < localStorage.length; i++) {
    const key = localStorage.key(i)
    if (key && SIZE_PREFIXES.some((prefix) => key.startsWith(prefix))) doomed.push(key)
  }
  for (const key of doomed) localStorage.removeItem(key)
  layout.seq += 1
}

loadSettings()

if (typeof matchMedia === 'function') {
  matchMedia('(prefers-color-scheme: light)').addEventListener?.('change', () => {
    if (settings.theme === 'system') applyDocument()
  })
}
