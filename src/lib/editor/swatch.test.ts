import { describe, expect, it } from 'vitest'
import { colorSpans, isImagePath, pickerHex, quotedStringAt, replaceHex } from './swatch'

describe('colorSpans', () => {
  it('finds 3, 6 and 8 digit colours inside quotes', () => {
    expect(colorSpans('    text "Hello" color "#fff"')).toEqual([{ from: 24, to: 28, hex: '#fff' }])
    expect(colorSpans("color '#aabbcc'")).toEqual([{ from: 7, to: 14, hex: '#aabbcc' }])
    expect(colorSpans('Solid("#aabbccdd")')[0].hex).toBe('#aabbccdd')
  })

  it('ignores colours in comments and outside strings', () => {
    expect(colorSpans('    # a note about #fff')).toEqual([])
    expect(colorSpans('    text #fff')).toEqual([])
  })

  it('keeps an escaped quote inside the string', () => {
    expect(colorSpans('"a \\" #ff00aa"')).toEqual([{ from: 6, to: 13, hex: '#ff00aa' }])
  })

  it('reads a one-line triple-quoted colour', () => {
    expect(colorSpans('"""#abc"""')).toEqual([{ from: 3, to: 7, hex: '#abc' }])
  })
})

describe('quotedStringAt', () => {
  it('returns the path under the caret', () => {
    const line = '    scene "images/bg.png"'
    const at = line.indexOf('bg')
    expect(quotedStringAt(line, at)?.text).toBe('images/bg.png')
    expect(isImagePath('images/bg.png')).toBe(true)
    expect(isImagePath('music/theme.ogg')).toBe(false)
  })
})

describe('pickerHex', () => {
  it('expands short forms and keeps alpha when writing back', () => {
    expect(pickerHex('#abc')).toBe('#aabbcc')
    expect(pickerHex('#AABBCCDD')).toBe('#aabbcc')
    expect(replaceHex('#AABBCCDD', '#112233')).toBe('#112233DD')
    expect(replaceHex('#fff', '#112233')).toBe('#112233')
  })
})
