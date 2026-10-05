/** Floating label for controls that already have an accessible name. */
export function tooltip(node: HTMLElement, text: string) {
  let label = text
  let pop: HTMLDivElement | null = null

  function place() {
    if (!pop) return
    const box = node.getBoundingClientRect()
    const width = pop.offsetWidth
    let left = box.left + box.width / 2 - width / 2
    left = Math.max(8, Math.min(left, window.innerWidth - width - 8))
    const below = box.bottom + 6
    const top = below + pop.offsetHeight > window.innerHeight - 8 ? box.top - pop.offsetHeight - 6 : below
    pop.style.left = `${left}px`
    pop.style.top = `${top}px`
  }

  function show() {
    if (!label || pop) return
    pop = document.createElement('div')
    pop.className = 'tip-pop'
    pop.setAttribute('role', 'tooltip')
    pop.textContent = label
    document.body.append(pop)
    place()
  }

  function hide() {
    pop?.remove()
    pop = null
  }

  node.addEventListener('mouseenter', show)
  node.addEventListener('mouseleave', hide)
  node.addEventListener('focus', show)
  node.addEventListener('blur', hide)
  return {
    update(next: string) {
      label = next
      if (pop) {
        pop.textContent = label
        place()
      }
    },
    destroy() {
      hide()
      node.removeEventListener('mouseenter', show)
      node.removeEventListener('mouseleave', hide)
      node.removeEventListener('focus', show)
      node.removeEventListener('blur', hide)
    },
  }
}
