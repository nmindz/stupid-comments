// Checks the Pi package wiring that no test can see: the `pi` manifest key,
// the extension it points at, and the keyword the Pi gallery discovers by.

import assert from 'node:assert/strict'
import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import { repo } from './versions.mjs'

const pkg = JSON.parse(readFileSync(join(repo, 'package.json'), 'utf8'))
const extensions = pkg.pi?.extensions
assert.ok(Array.isArray(extensions) && extensions.length > 0, 'package.json must declare pi.extensions to install as a Pi package')
assert.ok(pkg.keywords?.includes('pi-package'), 'the pi-package keyword lists the package in the Pi gallery')

for (const entry of extensions) {
  assert.ok(entry.startsWith('./'), `${entry} must be relative to the package root`)
  const path = join(repo, entry)
  assert.ok(existsSync(path), `pi.extensions names a missing module: ${entry}`)

  // Pi calls the default export as the factory and passes it the ExtensionAPI.
  const extension = await import(pathToFileURL(path).href)
  assert.equal(typeof extension.default, 'function', `${entry} must default-export the extension factory`)
}

// Pi supplies these to extensions; a bundled copy duplicates its registries.
const hostProvided = ['@earendil-works/pi-ai', '@earendil-works/pi-agent-core', '@earendil-works/pi-coding-agent', '@earendil-works/pi-tui', 'typebox']
for (const name of hostProvided) {
  assert.ok(!pkg.dependencies?.[name], `${name} is provided by Pi and must not be a dependency`)
}

console.log(`pi manifest: ok (v${pkg.version}, extensions ${extensions.join(', ')})`)
