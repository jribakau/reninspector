import { describe, expect, it } from 'vitest'
import { formatEdits, formatRenpy } from './format'

describe('formatRenpy', () => {
  it('turns leading tabs into 4 spaces and drops trailing whitespace', () => {
    expect(formatRenpy('\tlabel start:\t \n    e "Hi"  \n')).toBe('    label start:\n    e "Hi"\n')
  })

  it('leaves tabs and trailing spaces inside strings, including triple quotes', () => {
    const src = 'e "a\tb"  \ne """\n\thello  \n"""\n'
    expect(formatRenpy(src)).toBe('e "a\tb"\ne """\n\thello  \n"""\n')
  })

  it('does not touch a comment except its own trailing space', () => {
    expect(formatRenpy('    # keep\tthis  \n')).toBe('    # keep\tthis\n')
  })

  it('normalises only simple quotes, and only when asked', () => {
    const src = "e 'Hello'\ne \"It's\"\ne \"Hi [name]\"\ne \"a\\tb\"\n"
    expect(formatRenpy(src)).toBe(src)
    expect(formatRenpy(src, { quotes: 'double' })).toBe('e "Hello"\ne "It\'s"\ne "Hi [name]"\ne "a\\tb"\n')
  })

  it('is idempotent', () => {
    const src = '\tlabel start:\n    e "Hi"  \n\t\t"More"\n'
    const once = formatRenpy(src)
    expect(formatRenpy(once)).toBe(once)
    const quoted = formatRenpy(src, { quotes: 'single' })
    expect(formatRenpy(quoted, { quotes: 'single' })).toBe(quoted)
  })

  it('formats a selection without changing the lines outside it', () => {
    const src = '\tlabel a:\n\tlabel b:\n'
    const edits = formatEdits(src, {}, { fromLine: 2, toLine: 2 })
    expect(edits).toEqual([{ from: src.indexOf('\tlabel b'), to: src.indexOf('\tlabel b') + '\tlabel b:'.length, insert: '    label b:' }])
  })
})
