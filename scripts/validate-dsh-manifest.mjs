// Checks the wiring that no compiler or test can see: the DSH bundle manifest,
// the patch it points at, the module that patch inserts, and the version that
// five separate manifests each declare on their own.

import assert from 'node:assert/strict'
import { existsSync, readFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'
import { currentVersions, repo } from './versions.mjs'

const read = (path) => readFileSync(join(repo, path), 'utf8')
const readJson = (path) => JSON.parse(read(path))

const pkg = readJson('package.json')
const patchPath = pkg.dsh?.bundle?.patch
assert.ok(patchPath, 'package.json must declare dsh.bundle.patch to install as a DSH bundle')
assert.ok(existsSync(join(repo, patchPath)), `the declared bundle patch is missing: ${patchPath}`)

// Every module a patch inserts must exist. A relative name resolves against
// the patch file, which is how the plugin stays independent of its package name.
const patchDir = dirname(join(repo, patchPath))
const inserted = [...read(patchPath).matchAll(/^\s*-?\s*name:\s*['"]?([^'"\s]+)['"]?\s*$/gm)].map(m => m[1])
assert.ok(inserted.length > 0, 'the bundle patch inserts nothing')
for (const entry of inserted) {
  assert.ok(entry.startsWith('./') || entry.startsWith('../'), `${entry} must be a relative module name`)
  assert.ok(existsSync(resolve(patchDir, entry)), `the patch inserts a missing module: ${entry}`)
}

const plugin = await import(pathToFileURL(resolve(patchDir, inserted[0])).href)
assert.equal(typeof plugin.apply, 'function', 'the plugin must export apply()')
assert.equal(typeof plugin.name, 'string', 'the plugin must export a name')

assert.ok(existsSync(join(repo, pkg.main)), `package main is missing: ${pkg.main}`)

// The CC manifests, the DSH manifest, and the crate each carry their own copy
// of the version. Drift between them ships a plugin that lies about itself.
const versions = currentVersions()
const distinct = new Set(Object.values(versions))
assert.equal(distinct.size, 1, `version drift across manifests: ${JSON.stringify(versions, null, 2)}`)

console.log(`dsh manifest: ok (v${pkg.version}, inserts ${inserted.join(', ')})`)
