import type { Tooltip } from '@codemirror/view'
import type { DocEntry } from '../docs/reference'

export function tipButton(label: string, run: () => void): HTMLButtonElement {
  const button = document.createElement('button')
  button.type = 'button'
  button.textContent = label
  button.addEventListener('mousedown', (event) => event.preventDefault())
  button.addEventListener('click', run)
  return button
}

export function docText(text: string, from: number, to: number): Tooltip {
  return {
    pos: from,
    end: to,
    above: true,
    create() {
      const dom = document.createElement('div')
      dom.className = 'sym-tip'
      const body = document.createElement('div')
      body.textContent = text
      dom.append(body)
      return { dom }
    },
  }
}

export function docTip(doc: DocEntry, from: number, to: number): Tooltip {
  return {
    pos: from,
    end: to,
    above: true,
    create() {
      const dom = document.createElement('div')
      dom.className = 'sym-tip'
      const title = document.createElement('div')
      title.className = 'sym-title'
      title.textContent = doc.signature
      dom.append(title)
      const summary = document.createElement('div')
      summary.textContent = doc.summary
      dom.append(summary)
      const link = document.createElement('a')
      link.href = doc.url
      link.textContent = 'Documentation'
      link.target = '_blank'
      link.rel = 'noreferrer'
      dom.append(link)
      return { dom }
    },
  }
}
