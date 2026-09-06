// Writes one version into every manifest that carries it. semantic-release
// calls this from its prepare step; `@semantic-release/npm` has already put the
// number in package.json by then, and this makes the other four agree.
//
// Usage: node scripts/sync-version.mjs <version>

import assert from 'node:assert/strict'
import { currentVersions, manifests } from './versions.mjs'

const version = process.argv[2]
assert.match(version ?? '', /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/, 'pass a semver version to write')

for (const manifest of manifests) manifest.write(version)

const written = currentVersions()
for (const [path, value] of Object.entries(written)) {
  assert.equal(value, version, `${path} did not take the new version`)
}

console.log(`version: ${version} written to ${manifests.length} manifests`)
