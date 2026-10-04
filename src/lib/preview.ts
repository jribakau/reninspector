export type PreviewKind = 'image' | 'audio' | 'video' | 'text'

export function previewKind(name: string): PreviewKind | null {
  const ext = name.split('.').pop()?.toLowerCase() ?? ''
  switch (ext) {
    case 'png':
    case 'jpg':
    case 'jpeg':
    case 'gif':
    case 'webp':
    case 'avif':
    case 'bmp':
    case 'ico':
    case 'svg':
      return 'image'
    case 'mp3':
    case 'wav':
    case 'ogg':
    case 'opus':
    case 'flac':
    case 'm4a':
    case 'aac':
      return 'audio'
    case 'mp4':
    case 'webm':
    case 'ogv':
    case 'mov':
      return 'video'
    case 'txt':
    case 'md':
    case 'markdown':
    case 'rst':
    case 'log':
    case 'json':
    case 'yaml':
    case 'yml':
    case 'toml':
    case 'xml':
    case 'html':
    case 'htm':
    case 'css':
    case 'csv':
    case 'tsv':
    case 'ini':
    case 'cfg':
    case 'conf':
    case 'srt':
    case 'vtt':
    case 'py':
    case 'js':
    case 'mjs':
    case 'ts':
    case 'lua':
    case 'sh':
    case 'rpy':
    case 'rpym':
      return 'text'
    default:
      return null
  }
}

export function previewMime(name: string, kind: PreviewKind): string {
  const ext = name.split('.').pop()?.toLowerCase() ?? ''
  if (kind === 'image') {
    if (ext === 'jpg' || ext === 'jpeg') return 'image/jpeg'
    if (ext === 'webp') return 'image/webp'
    if (ext === 'gif') return 'image/gif'
    if (ext === 'avif') return 'image/avif'
    if (ext === 'bmp') return 'image/bmp'
    if (ext === 'ico') return 'image/x-icon'
    if (ext === 'svg') return 'image/svg+xml'
    return 'image/png'
  }
  if (kind === 'audio') {
    if (ext === 'mp3') return 'audio/mpeg'
    if (ext === 'wav') return 'audio/wav'
    if (ext === 'opus') return 'audio/ogg'
    if (ext === 'flac') return 'audio/flac'
    if (ext === 'm4a') return 'audio/mp4'
    if (ext === 'aac') return 'audio/aac'
    return 'audio/ogg'
  }
  if (kind === 'video') {
    if (ext === 'webm') return 'video/webm'
    if (ext === 'ogv') return 'video/ogg'
    if (ext === 'mov') return 'video/quicktime'
    return 'video/mp4'
  }
  return 'text/plain'
}
