import { EditorView } from '@codemirror/view'

type Spec = Record<string, Record<string, string | Record<string, string>>>

/** Surfaces every CodeMirror editor shares: canvas, gutters, selection, tooltips, search, completion. */
const shared: Spec = {
  '&': { height: '100%', backgroundColor: 'var(--bg-code)', color: 'var(--text)' },
  '.cm-scroller': { fontFamily: 'var(--code-font, var(--mono))', fontSize: 'var(--code-size, 13px)', lineHeight: 'var(--code-line, 1.55)' },
  '.cm-content': { caretColor: 'var(--accent)' },
  '.cm-gutters': {
    backgroundColor: 'var(--bg-code)',
    color: 'var(--dim)',
    border: 'none',
    borderRight: '1px solid var(--line-soft)',
  },
  '.cm-activeLine': { backgroundColor: 'color-mix(in srgb, var(--accent) 7%, transparent)' },
  '.cm-activeLineGutter': { backgroundColor: 'transparent', color: 'var(--text)' },
  '&.cm-focused': { outline: 'none' },
  '.cm-selectionBackground, &.cm-focused .cm-selectionBackground': {
    backgroundColor: 'color-mix(in srgb, var(--accent) 30%, transparent) !important',
  },
  '.cm-selectionMatch': { backgroundColor: 'color-mix(in srgb, var(--accent) 16%, transparent)' },
  '.cm-matchingBracket': {
    backgroundColor: 'color-mix(in srgb, var(--ok) 28%, transparent)',
    outline: '1px solid color-mix(in srgb, var(--ok) 55%, transparent)',
  },
  '.cm-nonmatchingBracket': {
    backgroundColor: 'color-mix(in srgb, var(--error) 28%, transparent)',
    outline: '1px solid color-mix(in srgb, var(--error) 55%, transparent)',
  },

  // Tooltips and completion
  '.cm-tooltip': {
    background: 'var(--panel)',
    color: 'var(--text)',
    border: '1px solid var(--line)',
    borderRadius: 'var(--r-md)',
    boxShadow: 'var(--shadow)',
    overflow: 'hidden',
  },
  '.cm-tooltip.autocomplete': { fontFamily: 'var(--mono)' },
  '.cm-tooltip-autocomplete > ul': { fontFamily: 'var(--mono)', maxHeight: '240px' },
  '.cm-tooltip-autocomplete > ul > li': { padding: '2px 10px', lineHeight: '1.5', color: 'var(--text)' },
  '.cm-tooltip-autocomplete > ul > li:hover': { backgroundColor: 'var(--hover)' },
  '.cm-tooltip-autocomplete > ul > li[aria-selected]': {
    backgroundColor: 'var(--sel)',
    color: 'var(--text)',
    boxShadow: 'inset 3px 0 0 var(--accent)',
  },
  '.cm-completionMatchedText': { textDecoration: 'none', color: 'var(--accent)', fontWeight: '600' },
  '.cm-completionDetail': { color: 'var(--dim)', fontStyle: 'normal', marginLeft: '0.8em' },
  '.cm-completionIcon': { opacity: '0.6' },

  // Search panel and matches
  '.cm-panels': {
    backgroundColor: 'var(--panel)',
    color: 'var(--text)',
    borderColor: 'var(--line)',
  },
  '.cm-panels-top': { borderBottom: '1px solid var(--line)' },
  '.cm-panels-bottom': { borderTop: '1px solid var(--line)' },
  '.cm-panel.cm-search': {
    background: 'var(--panel)',
    color: 'var(--text)',
    fontSize: 'var(--fs-md)',
    padding: '6px 28px 6px 10px',
  },
  '.cm-panel.cm-search input, .cm-panel.cm-search button, .cm-panel.cm-search .cm-textfield': {
    background: 'var(--bg)',
    color: 'var(--text)',
    border: '1px solid var(--line)',
    borderRadius: 'var(--r-sm)',
    fontSize: 'var(--fs-md)',
    padding: '2px 8px',
  },
  '.cm-panel.cm-search button': { backgroundImage: 'none', background: 'var(--panel-2)', cursor: 'pointer' },
  '.cm-panel.cm-search button:hover': { borderColor: 'var(--accent)' },
  '.cm-panel.cm-search input:focus': { outline: '1px solid var(--accent)', outlineOffset: '0' },
  '.cm-panel.cm-search label': { color: 'var(--dim)', fontSize: 'var(--fs-sm)' },
  '.cm-panel.cm-search [name=close]': {
    background: 'transparent',
    border: 'none',
    color: 'var(--dim)',
    fontSize: '16px',
    top: '4px',
    right: '6px',
  },
  '.cm-searchMatch': {
    backgroundColor: 'color-mix(in srgb, var(--warning) 26%, transparent)',
    outline: '1px solid color-mix(in srgb, var(--warning) 55%, transparent)',
  },
  '.cm-searchMatch.cm-searchMatch-selected': {
    backgroundColor: 'color-mix(in srgb, var(--accent) 38%, transparent)',
    outline: '1px solid var(--accent)',
  },
}

/** Colors for @codemirror/merge, which otherwise ships hardcoded pastel values. */
const merge: Spec = {
  '&.cm-merge-a .cm-changedLine, .cm-deletedChunk': {
    backgroundColor: 'color-mix(in srgb, var(--error) 9%, transparent) !important',
  },
  '&.cm-merge-b .cm-changedLine, .cm-inlineChangedLine': {
    backgroundColor: 'color-mix(in srgb, var(--ok) 9%, transparent) !important',
  },
  '&.cm-merge-a .cm-changedText, .cm-deletedChunk .cm-deletedText': {
    background: 'color-mix(in srgb, var(--error) 28%, transparent) !important',
    borderRadius: '2px',
  },
  '&.cm-merge-b .cm-changedText': {
    background: 'color-mix(in srgb, var(--ok) 28%, transparent) !important',
    borderRadius: '2px',
  },
  '&.cm-merge-b .cm-deletedText': { background: 'color-mix(in srgb, var(--error) 28%, transparent) !important' },
  '.cm-changedLineGutter, .cm-deletedLineGutter, .cm-inlineChangedLineGutter': { background: 'var(--accent) !important' },
  '&.cm-merge-a .cm-changedLineGutter, .cm-deletedLineGutter': { background: 'var(--error) !important' },
  '&.cm-merge-b .cm-changedLineGutter': { background: 'var(--ok) !important' },
  '.cm-collapsedLines': {
    color: 'var(--dim) !important',
    background: 'var(--panel-2) !important',
    borderRadius: 'var(--r-sm)',
    fontSize: 'var(--fs-sm)',
  },
}

/** The main script editor, including its gutters, git markers and live-line highlight. */
export function editorTheme(dark: boolean) {
  return EditorView.theme(
    {
      ...shared,
      '.cm-change-gutter': { width: '10px' },
      '.cm-change-gutter .cm-gutterElement': { padding: '0', display: 'flex', alignItems: 'stretch', justifyContent: 'center' },
      '.cm-change-gutter .git-bar': { display: 'block', width: '3px', height: '100%', margin: '0 auto', borderRadius: '1px' },
      '.cm-change-gutter .git-bar.added': { backgroundColor: 'var(--ok)' },
      '.cm-change-gutter .git-bar.modified': { backgroundColor: 'var(--accent)' },
      '.cm-change-gutter .git-bar.deleted': {
        width: 'auto',
        height: 'auto',
        alignSelf: 'center',
        backgroundColor: 'transparent',
        color: 'var(--error)',
        fontSize: 'var(--fs-xs)',
        lineHeight: '1',
        textAlign: 'center',
      },
      '.git-tip': {
        backgroundColor: 'var(--panel)',
        color: 'var(--text)',
        border: '1px solid var(--line)',
        borderRadius: 'var(--r-md)',
        boxShadow: 'var(--shadow)',
        padding: '8px',
        maxWidth: '440px',
      },
      '.git-tip pre': {
        margin: '0 0 8px',
        maxHeight: '120px',
        overflow: 'auto',
        color: 'var(--error)',
        fontFamily: 'var(--mono)',
        fontSize: 'var(--fs-md)',
        whiteSpace: 'pre',
      },
      '.git-tip .row': { display: 'flex', gap: '6px' },
      '.git-tip button': { fontSize: 'var(--fs-sm)', padding: '3px 8px' },
      '.cm-live-line': { backgroundColor: 'color-mix(in srgb, var(--ok) 18%, transparent)' },
      '.cm-live-gutter': { width: '14px', color: 'var(--ok)' },
      '.cm-ctrl-link': {
        cursor: 'pointer',
        textDecorationLine: 'underline',
        textDecorationColor: 'var(--accent)',
        textUnderlineOffset: '2px',
      },
      '&.cm-ctrl-held .cm-tooltip-hover': { display: 'none' },
      '.cm-ctrl-tip': { padding: '6px 0', maxWidth: '420px', fontSize: 'var(--fs-md)' },
      '.cm-ctrl-tip .head': { padding: '0 8px 4px', fontWeight: '600' },
      '.cm-ctrl-tip .where': { color: 'var(--dim)', fontSize: 'var(--fs-sm)' },
      '.cm-ctrl-tip .head + .where': { padding: '0 8px 4px' },
      '.cm-ctrl-tip button': {
        display: 'grid',
        width: '100%',
        textAlign: 'left',
        gap: '2px',
        padding: '3px 8px',
        border: 'none',
        borderRadius: '0',
        background: 'transparent',
        color: 'var(--text)',
        font: 'inherit',
      },
      '.cm-ctrl-tip button .line': {
        overflow: 'hidden',
        textOverflow: 'ellipsis',
        whiteSpace: 'nowrap',
      },
      '.cm-ctrl-tip button:hover': { background: 'var(--hover)', borderColor: 'transparent' },
      '.cm-ctrl-tip button.more': { color: 'var(--accent)' },
      '.sym-tip': { padding: '6px 8px', maxWidth: '360px', fontSize: 'var(--fs-md)' },
      '.sym-title': { fontWeight: '600', marginBottom: '2px' },
      '.sym-tip img': { display: 'block', marginTop: '6px', maxWidth: '220px', maxHeight: '140px' },
      '.cm-range': { backgroundColor: 'color-mix(in srgb, var(--accent) 7%, transparent)' },
      '.cm-range-start': {
        backgroundColor: 'color-mix(in srgb, var(--accent) 22%, transparent)',
        boxShadow: 'inset 3px 0 0 var(--accent)',
      },
      '.cm-diag-gutter': { width: '14px' },
    },
    { dark },
  )
}

/** Read-only diff views. Same canvas as the editor plus the merge chunk colors. */
export function diffTheme(dark: boolean) {
  return EditorView.theme({ ...shared, ...merge }, { dark })
}
