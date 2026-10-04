// One place for every glyph in the UI. All icons are drawn on a 16x16 grid.
// `fill` icons are solid shapes; the rest are 1.4px round-capped strokes.

export interface IconDef {
  d: string
  fill?: boolean
}

const fill = (d: string): IconDef => ({ d, fill: true })
const line = (d: string): IconDef => ({ d })

export const ICONS = {
  // Navigation and chrome
  menu: fill('M2 3.25h12v1.5H2zm0 4h12v1.5H2zm0 4h12v1.5H2z'),
  caret: fill('M4.2 6.2h7.6L8 10.4z'),
  'chevron-right': line('M6 3.5 10.5 8 6 12.5'),
  'chevron-down': line('M3.5 6 8 10.5 12.5 6'),
  'chevron-up': line('M3.5 10 8 5.5 12.5 10'),
  'chevron-left': line('M10 3.5 5.5 8l4.5 4.5'),
  'arrow-up': line('M8 13V3M4 7l4-4 4 4'),
  'arrow-down': line('M8 3v10M4 9l4 4 4-4'),
  'arrow-right': line('M3 8h10M9 4l4 4-4 4'),
  close: line('M3.5 3.5l9 9M12.5 3.5l-9 9'),
  check: line('M3 8.5 6.5 12 13 4.5'),
  plus: line('M8 3v10M3 8h10'),
  minus: line('M3 8h10'),
  fit: line('M2.5 6V2.5H6M10 2.5h3.5V6M13.5 10v3.5H10M6 13.5H2.5V10'),
  more: fill('M3 6.5a1.5 1.5 0 1 1 0 3 1.5 1.5 0 0 1 0-3zm5 0a1.5 1.5 0 1 1 0 3 1.5 1.5 0 0 1 0-3zm5 0a1.5 1.5 0 1 1 0 3 1.5 1.5 0 0 1 0-3z'),

  // Window controls
  minimize: fill('M3 7.4h10v1.2H3z'),
  maximize: fill('M3.2 3.2h9.6v9.6H3.2V3.2zm1.2 1.2v7.2h7.2V4.4H4.4z'),
  restore: fill('M5 5.2h6.2V4H4v7.2h1.2V5.2zM6.2 6.2H12V12H6.2V6.2zm1.1 1.1v3.6h3.6V7.3H7.3z'),
  'window-close': fill('m4.1 3.4 3.9 3.9 3.9-3.9 1.1 1.1L9.1 8.4l3.9 3.9-1.1 1.1-3.9-3.9-3.9 3.9-1.1-1.1 3.9-3.9-3.9-3.9z'),

  // Activities
  explorer: fill('M1.5 2.5h5l1.2 1.3H14.5v9.7h-13V2.5zm1 2.3v7.7h11V4.8H7.2L6 3.5H2.5v1.3z'),
  folder: fill('M1.5 3.2h4.6l1.2 1.3H14.5v8H1.5v-9.3zm1.2 2.4v5.7h10.6V5.6H6.6L5.4 4.4H2.7v1.2z'),
  story: fill('M2 3.2h5.2v2.1H2V3.2zm6.8 0H14v2.1H8.8V3.2zM2 7h4.2v2.1H2V7zm5.8 0H14v2.1H7.8V7zM2 10.8h6.2V13H2v-2.2zm7.8 0H14V13H9.8v-2.2z'),
  search: fill('M7 2a5 5 0 0 1 3.9 8.1l3 3-1.1 1.1-3-3A5 5 0 1 1 7 2zm0 1.4a3.6 3.6 0 1 0 0 7.2 3.6 3.6 0 0 0 0-7.2z'),
  renpy: fill('M2 3.2h12v7.2H9.2L6.4 13v-2.6H2V3.2zm1.2 1.2v4.8h4.2V12l2-2.8h4.4V4.4H3.2z'),
  branch: fill('M6.2 2.2a1.6 1.6 0 0 0-1 2.9v5.8a1.6 1.6 0 1 0 1.2 0V8.3c.7.4 1.5.6 2.4.6h.2a1.6 1.6 0 1 0 0-1.2h-.2c-1.2 0-2.1-.4-2.6-1.1V5.1a1.6 1.6 0 0 0 0-2.9zM5.2 3.2a.6.6 0 1 1 0 1.2.6.6 0 0 1 0-1.2zm0 8.4a.6.6 0 1 1 0 1.2.6.6 0 0 1 0-1.2zm5.2-3.6a.6.6 0 1 1 0 1.2.6.6 0 0 1 0-1.2z'),
  panel: line('M2 3h12v10H2zM2 10h12'),
  split: line('M2 3h12v10H2zM10 3v10'),

  // Run
  play: fill('M5 3.2v9.6l7.2-4.8z'),
  stop: fill('M4 4h8v8H4z'),

  // Files and views
  file: line('M3.5 1.5h6l3 3v10h-9zM9.5 1.5v3h3'),
  code: line('M5.5 4 1.8 8l3.7 4M10.5 4l3.7 4-3.7 4'),
  flow: line('M2.5 2.5h4v3.5h-4zM9.5 10h4v3.5h-4zM4.5 6v2.5h7V10'),
  stage: line('M2 3h12v8H2zM5 13.5h6'),
  map: line('M1.5 3.5 5.5 2l5 1.5 4-1.5v10.5l-4 1.5-5-1.5-4 1.5zM5.5 2v10.5M10.5 3.5V14'),
  diff: line('M4.5 2.5v5M2 5h5M9.5 11h5'),
  image: line('M2 3h12v10H2zM2 11l3.5-3.5 3 3 2-2L14 12'),
  audio: line('M2.5 6h2.5L8.5 3v10L5 10H2.5zM11 5.5a3.5 3.5 0 0 1 0 5'),
  video: line('M2 4h8v8H2zM10 7l4-2.5v7L10 9'),
  archive: line('M2 3h12v3H2zM3 6v7h10V6M6.5 9h3'),
  'file-plus': line('M3.5 1.5h6l3 3v10h-9zM9.5 1.5v3h3M8 7.5v4M6 9.5h4'),
  'folder-plus': line('M1.5 3.5h4.5l1.3 1.5h7.2v8h-13zM8 7v4M6 9h4'),
  'folder-open': line('M1.5 13V3.5h4.5l1.3 1.5h5.2v2.2M1.5 13l2-5.8h11.5l-2 5.8z'),
  'file-text': line('M3.5 1.5h6l3 3v10h-9zM9.5 1.5v3h3M5.5 8h5M5.5 10.5h5'),
  markdown: line('M1.5 3.5h13v9h-13zM3.8 10V6l2 2.3L7.8 6v4M11 6v4M9.5 8.5 11 10l1.5-1.5'),
  config: line('M2 4.5h12M2 11.5h12M5.5 3v3M10.5 10v3'),
  lock: line('M4 7h8v6.5H4zM5.5 7V5a2.5 2.5 0 0 1 5 0v2'),
  locate: line('M8 2.5v2.5M8 11v2.5M2.5 8H5M11 8h2.5M8 5.7a2.3 2.3 0 1 0 0 4.6 2.3 2.3 0 0 0 0-4.6z'),
  settings: line('M8 5.7a2.3 2.3 0 1 0 0 4.6 2.3 2.3 0 0 0 0-4.6zM8 1.8v2M8 12.2v2M1.8 8h2M12.2 8h2M3.6 3.6 5 5M11 11l1.4 1.4M12.4 3.6 11 5M5 11l-1.4 1.4'),
  font: line('M3 13 8 3l5 10M5 9.5h6'),

  // Status
  error: line('M8 2a6 6 0 1 0 0 12A6 6 0 0 0 8 2zM5.9 5.9l4.2 4.2M10.1 5.9l-4.2 4.2'),
  warning: line('M8 2.5 14.2 13.5H1.8zM8 6.6v3M8 11.6v.01'),
  info: line('M8 2a6 6 0 1 0 0 12A6 6 0 0 0 8 2zM8 7.3v3.8M8 4.9v.01'),
  dot: fill('M8 5a3 3 0 1 1 0 6 3 3 0 0 1 0-6z'),
  live: fill('M8 6a2 2 0 1 1 0 4 2 2 0 0 1 0-4zM4.2 4.2l.9.9a4 4 0 0 0 0 5.8l-.9.9a5.3 5.3 0 0 1 0-7.6zm7.6 0a5.3 5.3 0 0 1 0 7.6l-.9-.9a4 4 0 0 0 0-5.8z'),

  // Actions
  filter: line('M2.5 3.5h11L9.5 8.5V13l-3-1.5V8.5z'),
  refresh: line('M13 8a5 5 0 1 1-1.5-3.5M13 2.5v3h-3'),
  copy: line('M5.5 5.5h7v8h-7zM3.5 10.5v-8h7'),
  trash: line('M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.5 9h6l.5-9'),
  history: line('M8 2.5a5.5 5.5 0 1 0 0 11 5.5 5.5 0 0 0 0-11zM8 5v3.2l2 1.3'),
  edit: line('M10.5 2.5l3 3-8 8H2.5v-3z'),
  save: line('M3 2.5h8.5l1.5 1.5v9.5H3zM5.5 2.5v3.5h5V2.5M5.5 13.5V9h5v4.5'),
  undo: line('M4 6h5.5a3.5 3.5 0 0 1 0 7H6M6.5 3.5 4 6l2.5 2.5'),
  redo: line('M12 6H6.5a3.5 3.5 0 0 0 0 7H10M9.5 3.5 12 6 9.5 8.5'),
  eye: line('M1.5 8S4 3.5 8 3.5 14.5 8 14.5 8 12 12.5 8 12.5 1.5 8 1.5 8zM8 6.2a1.8 1.8 0 1 0 0 3.6 1.8 1.8 0 0 0 0-3.6z'),
  pin: line('M9.5 2.5l4 4-2 .5-2.5 2.5.5 3-1 1-6-6 1-1 3 .5L9 4.5zM2.5 13.5l3-3'),
} satisfies Record<string, IconDef>

export type IconName = keyof typeof ICONS
