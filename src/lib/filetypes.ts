import type { IconName } from './icons'
import { previewKind } from './preview'

export type FileTone = 'script' | 'image' | 'audio' | 'video' | 'doc' | 'config' | 'archive' | 'plain'

export interface FileLook {
  icon: IconName
  tone: FileTone
}

const FONTS = new Set(['ttf', 'otf', 'woff', 'woff2', 'ttc'])
const CONFIG = new Set(['json', 'yaml', 'yml', 'toml', 'ini', 'cfg', 'conf', 'xml'])
const ARCHIVES = new Set(['rpa', 'rpyc', 'rpymc', 'zip', '7z', 'rar', 'tar', 'gz'])
const CODE = new Set(['py', 'js', 'mjs', 'ts', 'lua', 'sh', 'html', 'htm', 'css'])

export function extOf(name: string): string {
  const dot = name.lastIndexOf('.')
  return dot <= 0 ? '' : name.slice(dot + 1).toLowerCase()
}

/** Icon and tone for a file name in the explorer. */
export function fileLook(name: string): FileLook {
  const ext = extOf(name)
  if (ext === 'rpy' || ext === 'rpym') return { icon: 'renpy', tone: 'script' }
  if (ARCHIVES.has(ext)) return { icon: 'archive', tone: 'archive' }
  if (ext === 'md' || ext === 'markdown') return { icon: 'markdown', tone: 'doc' }
  if (CONFIG.has(ext)) return { icon: 'config', tone: 'config' }
  if (CODE.has(ext)) return { icon: 'code', tone: 'config' }
  if (FONTS.has(ext)) return { icon: 'font', tone: 'doc' }
  const kind = previewKind(name)
  if (kind === 'image') return { icon: 'image', tone: 'image' }
  if (kind === 'audio') return { icon: 'audio', tone: 'audio' }
  if (kind === 'video') return { icon: 'video', tone: 'video' }
  if (kind === 'text') return { icon: 'file-text', tone: 'doc' }
  return { icon: 'file', tone: 'plain' }
}
