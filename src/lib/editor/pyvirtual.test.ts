import { describe, expect, it } from 'vitest'
import { inPython } from './python'
import { toVirtual } from './pyvirtual'

const src = `label start:
    e "Hi"
init python:
    def greet(who):
        return who
    # still python
    x = 1

label later:
    $ points += 1
    e "Next"
python early:
    class Box:
        pass
define e = Character(None)
define -1 points = 0
`

describe('toVirtual', () => {
  const virt = toVirtual(src)
  const srcLines = src.split('\n')
  const outLines = virt.split('\n')

  it('keeps one line per source line', () => {
    expect(outLines.length).toBe(srcLines.length)
  })

  it('turns headers into valid Python and keeps the body in column', () => {
    expect(outLines[2]).toBe('if 1:' + ' '.repeat('init python:'.length - 'if 1:'.length))
    expect(outLines[3]).toBe(srcLines[3])
    expect(outLines[3].indexOf('greet')).toBe(srcLines[3].indexOf('greet'))
    expect(outLines[11]).toBe('if 1:' + ' '.repeat('python early:'.length - 'if 1:'.length))
    expect(outLines[12]).toBe(srcLines[12])
  })

  it('keeps a dollar line in column and drops dialogue', () => {
    expect(outLines[9].indexOf('points')).toBe(srcLines[9].indexOf('points'))
    expect(outLines[9].startsWith('    ')).toBe(true)
    expect(outLines[0]).toBe('')
    expect(outLines[1]).toBe('')
    expect(outLines[8]).toBe('')
    expect(outLines[10]).toBe('')
  })

  it('blanks define and default but keeps the assignment', () => {
    expect(outLines[14].indexOf('e =')).toBe(srcLines[14].indexOf('e ='))
    expect(outLines[14].trimStart().startsWith('e =')).toBe(true)
    expect(outLines[15].indexOf('points')).toBe(srcLines[15].indexOf('points'))
  })

  it('keeps the continuation lines of a multi-line define or dollar statement', () => {
    const multi = [
      'define e = Character(',
      '    "Eileen",',
      '    color="#fff",',
      ')',
      'label start:',
      '    $ total = add(1,',
      '                  2)',
      '    e "Hi (not code"',
      'default scores = [1, \\',
      '    2]',
      'define dialogue = "("',
      'label end:',
    ].join('\n')
    const got = toVirtual(multi).split('\n')
    expect(got.length).toBe(12)
    expect(got[1]).toBe('    "Eileen",')
    expect(got[2]).toBe('    color="#fff",')
    expect(got[3]).toBe(')')
    expect(got[6]).toBe('                  2)')
    // The dialogue after a balanced statement is dropped again.
    expect(got[7]).toBe('')
    expect(got[9]).toBe('    2]')
    // A bracket inside a string does not start a continuation.
    expect(got[10].trimStart()).toBe('dialogue = "("')
    expect(got[11]).toBe('')
  })

  it('rewrites Python 2 without moving the tokens that stay', () => {
    const py2 = [
      'label start:',
      'init python:',
      '    try:',
      '        print value',
      '    except ValueError, err:',
      '        x = 1 <> 0',
      '        n = 1L',
      '        s = ur"hi"',
      '        raise ValueError, err',
      '        y = xrange(n)',
      '        z = `gone`',
    ].join('\n')
    const srcLines = py2.split('\n')
    const got = toVirtual(py2, true).split('\n')
    expect(got).toHaveLength(srcLines.length)
    expect(got[0]).toContain('from renpy7_compat import ')
    expect(got[4]).toBe('    except ValueError     :')
    expect(got[4].indexOf('ValueError')).toBe(srcLines[4].indexOf('ValueError'))
    expect(got[5]).toBe('        x = 1 != 0')
    expect(got[6]).toBe('        n = 1 ')
    expect(got[7]).toBe('        s =  r"hi"')
    expect(got[8]).toBe('        raise ValueError     ')
    expect(got[8]).toHaveLength(srcLines[8].length)
    expect(got[3].trim()).toBe('')
    expect(got[3]).toHaveLength(srcLines[3].length)
    expect(got[9].indexOf('xrange')).toBe(srcLines[9].indexOf('xrange'))
    expect(got[10].trim()).toBe('')
    expect(got.every((line, i) => i === 0 || line.length === srcLines[i].length)).toBe(true)
  })

  it('puts the compat import on the header when the file starts with python', () => {
    const src = 'init python:\n    x = xrange(1)\n'
    const got = toVirtual(src, true).split('\n')
    expect(got[0].startsWith('from renpy7_compat import ')).toBe(true)
    expect(got[0].endsWith('; if 1:')).toBe(true)
    expect(got[1].indexOf('xrange')).toBe('    x = xrange(1)'.indexOf('xrange'))
  })

  it('leaves a Python 3 script alone when the dialect flag is off', () => {
    const line = '    except ValueError as err:'
    expect(toVirtual(`init python:\n${line}\n`).split('\n')[1]).toBe(line)
  })

  it('only keeps text on lines python.ts calls inline, plus headers and defines', () => {
    const doc = { lines: srcLines.length, line: (n: number) => ({ text: srcLines[n - 1] ?? '' }) }
    outLines.forEach((line, i) => {
      if (!line.trim()) return
      const header = line.startsWith('if 1:')
      const defined = srcLines[i].trimStart().startsWith('define') || srcLines[i].trimStart().startsWith('default')
      expect(header || defined || inPython(doc, i + 1), `line ${i + 1}: ${line}`).toBe(true)
    })
  })
})
