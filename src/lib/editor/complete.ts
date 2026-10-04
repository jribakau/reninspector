import { snippetCompletion, type Completion, type CompletionContext, type CompletionResult } from '@codemirror/autocomplete'
import { api } from '../api'
import { symbolsOf } from '../indexes.svelte'
import { app } from '../model.svelte'

const KEYWORDS = [
  'label', 'menu', 'jump', 'call', 'return', 'if', 'elif', 'else', 'while', 'pass', 'show', 'scene',
  'hide', 'with', 'define', 'default', 'image', 'play', 'stop', 'queue', 'voice', 'pause', 'screen',
  'translate', 'python', 'init', 'transform', 'nvl', 'window show', 'window hide', 'extend', 'centered',
  'call screen', 'show screen', 'hide screen',
]

const TRANSITIONS = [
  'dissolve', 'fade', 'None', 'pixellate', 'blinds', 'squares', 'wipeleft', 'wiperight', 'wipeup', 'wipedown',
  'Dissolve', 'Fade', 'Pixellate', 'move', 'ease', 'vpunch', 'hpunch', 'irisin', 'irisout', 'zoomin', 'zoomout',
]

const SNIPPETS: Completion[] = [
  snippetCompletion('label ${name}:\n    "${dialogue}"\n    return', { label: 'label', detail: 'snippet', type: 'keyword' }),
  snippetCompletion(
    'menu ${name}:\n    "${choice1}":\n        jump ${target1}\n    "${choice2}":\n        jump ${target2}',
    { label: 'menu', detail: 'snippet', type: 'keyword' },
  ),
  snippetCompletion('if ${condition}:\n    ${then}\nelse:\n    ${otherwise}', { label: 'if', detail: 'snippet', type: 'keyword' }),
  snippetCompletion('define ${name} = Character("${display}")', { label: 'define', detail: 'snippet', type: 'keyword' }),
  snippetCompletion('default ${name} = ${value}', { label: 'default', detail: 'snippet', type: 'keyword' }),
  snippetCompletion('screen ${name}():\n    ${body}', { label: 'screen', detail: 'snippet', type: 'keyword' }),
  snippetCompletion('transform ${name}:\n    ${body}', { label: 'transform', detail: 'snippet', type: 'keyword' }),
  snippetCompletion('scene ${image} with fade', { label: 'scene', detail: 'snippet', type: 'keyword' }),
  snippetCompletion('show ${image} at ${place} with dissolve', { label: 'show', detail: 'snippet', type: 'keyword' }),
]

let audioCache: { seq: number; paths: string[] } | null = null

async function audioPaths(): Promise<string[]> {
  const seq = app.changeSeq
  if (audioCache?.seq === seq) return audioCache.paths
  try {
    const report = await api.assetReport()
    const paths = report.files.filter((f) => f.kind === 'audio').map((f) => f.path)
    audioCache = { seq, paths }
    return paths
  } catch {
    return audioCache?.paths ?? []
  }
}

function named(kind: string, detail: string, type: string): Completion[] {
  return symbolsOf(kind).map((s) => ({ label: s.name, detail: s.path || detail, type }))
}

function variables(): Completion[] {
  return (app.catalog?.variables ?? []).map((v) => ({ label: v.name, detail: v.keyword || 'variable', type: 'variable' }))
}

function keywords(): Completion[] {
  return KEYWORDS.map((k) => ({ label: k, detail: 'statement', type: 'keyword' }))
}

function pick(pool: Completion[], prefix: string): Completion[] {
  const pre = prefix.toLowerCase()
  if (!pre) return pool.slice(0, 40)
  const starts = pool.filter((o) => o.label.toLowerCase().startsWith(pre))
  const list = starts.length ? starts : pool.filter((o) => o.label.toLowerCase().includes(pre))
  return list.slice(0, 40)
}

function quotedAudio(path: string, quoted: boolean): Completion {
  return {
    label: path,
    detail: 'audio',
    type: 'text',
    apply: quoted
      ? undefined
      : (view, _completion, from, to) => {
          const insert = `"${path}"`
          view.dispatch({
            changes: { from, to, insert },
            selection: { anchor: from + insert.length },
          })
        },
  }
}

/** Ren'Py-aware completions: names from the project, plus snippets at the start of a statement. */
export async function renpyComplete(context: CompletionContext): Promise<CompletionResult | null> {
  const line = context.state.doc.lineAt(context.pos)
  const before = line.text.slice(0, context.pos - line.from)
  const word = context.matchBefore(/[A-Za-z_][\w.]*/)
  const pathWord = context.matchBefore(/[\w./\\-]+/)
  if (!word && !pathWord && !context.explicit) return null

  const atStatement = /^\s*[A-Za-z_]*$/.test(before)
  let from = word ? word.from : context.pos
  let prefix = word?.text ?? ''
  let pool: Completion[] = []
  let validFor = /^[\w.]*$/

  if (/\b(?:call|show|hide)\s+screen\s+[\w.]*$/.test(before)) {
    pool = named('screen', 'screen', 'screen')
  } else if (/\b(?:play|queue)\s+(?:music|sound|voice|audio|ambient)\s+["']?[\w./\\-]*$/.test(before) || /\bvoice\s+["']?[\w./\\-]*$/.test(before)) {
    const quoted = /["'][\w./\\-]*$/.test(before)
    from = pathWord && /[\w./\\-]/.test(before.slice(-1)) ? pathWord.from : context.pos
    prefix = pathWord && from === pathWord.from ? pathWord.text : ''
    const paths = await audioPaths()
    pool = paths.map((p) => quotedAudio(p, quoted))
    validFor = /^[\w./\\-]*$/
  } else if (/\bwith\s+[\w.]*$/.test(before)) {
    pool = TRANSITIONS.map((t) => ({ label: t, detail: 'transition', type: 'constant' }))
  } else if (/\bat\s+[\w.]*$/.test(before)) {
    pool = named('transform', 'transform', 'function')
  } else if (/(?:^|\s)\$(?:\s*[\w.]*)?$/.test(before) || /\b(?:if|elif|while)\s+[\w.]*$/.test(before) || /\b(?:define|default)\b[^=\n]*=\s*[\w.]*$/.test(before)) {
    pool = variables()
  } else if (/\b(?:jump|call|menu)\s+[\w.]*$/.test(before)) {
    pool = named('label', 'label', 'label')
  } else if (/\b(?:show|scene|hide)\s+(?!screen\b)[\w ]*$/.test(before)) {
    pool = named('image', 'image', 'image')
    const imageTail = before.match(/(?:show|scene|hide)\s+([\w ]*)$/)
    if (imageTail) {
      prefix = imageTail[1]
      from = context.pos - prefix.length
      validFor = /^[\w ]*$/
    }
  } else if (atStatement) {
    pool = [
      ...SNIPPETS,
      ...symbolsOf('character').map((s) => ({ label: s.name, detail: s.detail || 'character', type: 'variable' })),
      ...keywords(),
    ]
  } else {
    pool = keywords()
  }

  const options = pick(pool, prefix)
  if (!options.length) return null
  return { from, options, validFor }
}
