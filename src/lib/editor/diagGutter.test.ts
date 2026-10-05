import { describe, expect, it } from 'vitest'
import { diagnosticMarks } from './diagGutter'
import type { Diagnostic } from '../types'

const diag = (line: number, severity: Diagnostic['severity'], message: string): Diagnostic => ({
  line,
  severity,
  message,
  path: 'script.rpy',
  code: '',
  label: null,
})

describe('diagnostic marks', () => {
  it('keeps the strongest project diagnostic and drops info', () => {
    const marks = diagnosticMarks(
      [diag(2, 'warning', 'soft'), diag(2, 'error', 'hard'), diag(3, 'info', 'note')],
      new Map(),
      [],
      'all',
      () => true,
    )
    expect(marks.get(2)).toEqual({ severity: 'error', message: 'hard' })
    expect(marks.has(3)).toBe(false)
  })

  it('lets a syntax error replace a warning, and filters python by level', () => {
    const marks = diagnosticMarks(
      [diag(1, 'warning', 'soft')],
      new Map([[1, 'broken']]),
      [
        { line: 4, message: 'typo', severity: 'error', code: 'undefined' },
        { line: 5, message: 'bad', severity: 'error', code: 'invalid-syntax' },
      ],
      'syntax',
      (line) => line >= 4,
    )
    expect(marks.get(1)?.message).toBe('broken')
    expect(marks.has(4)).toBe(false)
    expect(marks.get(5)?.message).toBe('bad')
  })
})