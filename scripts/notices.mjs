// Writes THIRD_PARTY_NOTICES.md from the Rust and npm dependencies that ship in the app.
// Run with `npm run notices` after changing dependencies.
import { execFileSync } from 'node:child_process'
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const own = new Set(['vn-ide', 'renpy-core'])

function rustCrates() {
  const out = execFileSync(
    'cargo',
    ['tree', '-e', 'normal', '--target', 'all', '--prefix', 'none', '--format', '{p}|{l}|{r}'],
    { cwd: join(root, 'src-tauri'), encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] },
  )
  const seen = new Map()
  for (const line of out.split(/\r?\n/)) {
    const [pkg, license = '', repo = ''] = line.replace(/\s*\(\*\)$/, '').split('|')
    if (!pkg || pkg.includes('(proc-macro)')) continue
    const m = pkg.match(/^(\S+) v(\S+)/)
    if (!m || own.has(m[1])) continue
    seen.set(`${m[1]} ${m[2]}`, { name: m[1], version: m[2], license: license || 'see source', repo })
  }
  return [...seen.values()].sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version))
}

function npmPackages() {
  const lock = JSON.parse(readFileSync(join(root, 'package-lock.json'), 'utf8'))
  const out = []
  for (const [path, entry] of Object.entries(lock.packages ?? {})) {
    if (!path || entry.dev) continue
    const manifest = join(root, path, 'package.json')
    const pkg = existsSync(manifest) ? JSON.parse(readFileSync(manifest, 'utf8')) : {}
    const repo = typeof pkg.repository === 'string' ? pkg.repository : (pkg.repository?.url ?? '')
    out.push({
      name: path.replace(/^.*node_modules\//, ''),
      version: entry.version ?? pkg.version ?? '',
      license: entry.license ?? pkg.license ?? 'see source',
      repo: repo.replace(/^git\+/, '').replace(/\.git$/, ''),
    })
  }
  return out.sort((a, b) => a.name.localeCompare(b.name))
}

function table(rows) {
  const lines = ['| Package | Version | Licence | Source |', '|---|---|---|---|']
  for (const r of rows) lines.push(`| ${r.name} | ${r.version} | ${r.license} | ${r.repo} |`)
  return lines.join('\n')
}

const text = `# Third-party notices

This application includes the third-party software listed below. Each package
is distributed under its own licence, linked from its source repository.

## Licences with extra conditions

- **elkjs** (EPL-2.0). Graph layout. Source: https://github.com/kieler/elkjs.
  Used unmodified. It does not grant the GPL as a secondary licence, so it stays
  under the EPL-2.0. The project's \`LICENSE\` gives a GPL section 7 permission
  to ship it alongside the GPL code.
- **spellbook, cssparser, selectors, dtoa-short, option-ext** (MPL-2.0). Source
  for each is linked in the table below; the MPL-2.0 files are used unmodified.
- **English spell-check dictionary** (en_US Hunspell dictionary from SCOWL,
  http://wordlist.aspell.net/). Copyright Kevin Atkinson and others; the full
  licence and copyright notices are in \`licenses/SCOWL.txt\`.
- **ICU4X crates** (Unicode-3.0) and **webpki-roots** (CDLA-Permissive-2.0).
- **unrpa** (GPL-3.0, https://github.com/Lattyware/unrpa). The RPA-3.2, RPA-4.0,
  ALT-1.0 and ZiX-12A/B header layouts, key derivation and index handling in
  \`renpy-core/src/rpa\` were reimplemented in Rust from its \`versions/\` modules.
  No unrpa code is included.

## Rust crates

Crates used only on other operating systems are included so one list covers
every build.

${table(rustCrates())}

## JavaScript packages

${table(npmPackages())}
`

writeFileSync(join(root, 'THIRD_PARTY_NOTICES.md'), text)
console.log('Wrote THIRD_PARTY_NOTICES.md')
