// The version lives in five files that no build step ties together. This is the
// one list of them, shared by the writer (sync-version) and the check that
// fails a build on drift.

import { readFileSync, writeFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

export const repo = resolve(fileURLToPath(new URL('../', import.meta.url)))

const read = (path) => readFileSync(join(repo, path), 'utf8')
const write = (path, text) => writeFileSync(join(repo, path), text)

/**
 * A JSON manifest carrying the version at `pointer`. The value is located by
 * parsing and rewritten textually, because reserializing would reformat a
 * hand-written manifest on every release.
 */
function jsonManifest(path, pointer = (doc) => doc) {
  const current = () => pointer(JSON.parse(read(path))).version
  return {
    path,
    read: current,
    write: (version) => textManifest(path, quoted(current())).write(version),
  }
}

function quoted(version) {
  return new RegExp(`"version"\\s*:\\s*"(${version.replace(/[.+*?^$()[\]{}|\\]/g, '\\$&')})"`)
}

/** A TOML or lock entry matched by pattern, rewritten through its capture. */
function textManifest(path, pattern) {
  return {
    path,
    read: () => read(path).match(pattern)?.[1],
    write: (version) => {
      const text = read(path)
      const match = text.match(pattern)
      if (!match) throw new Error(`${path}: no version to rewrite`)
      const at = match.index + match[0].indexOf(match[1])
      write(path, text.slice(0, at) + version + text.slice(at + match[1].length))
    },
  }
}

export const manifests = [
  jsonManifest('package.json'),
  jsonManifest('plugins/stupid-comments/.claude-plugin/plugin.json'),
  jsonManifest('.claude-plugin/marketplace.json', doc => doc.plugins[0]),
  textManifest('Cargo.toml', /\[workspace\.package\][\s\S]*?version\s*=\s*"([^"]+)"/),
  textManifest('Cargo.lock', /name = "stupid-comments"\nversion = "([^"]+)"/),
]

/** Every manifest's current version, keyed by path. */
export function currentVersions() {
  return Object.fromEntries(manifests.map(m => [m.path, m.read()]))
}
