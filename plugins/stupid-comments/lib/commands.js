// Slash command prompts. The bodies are the same markdown files Claude Code
// loads, so no harness owns a private copy of the wording.

import { readdirSync, readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const COMMANDS_DIR = new URL('../commands/', import.meta.url)

/** Every `commands/*.md` as `{ slug, description, hint?, body }`. */
export function loadCommands(warn = () => {}) {
  let entries
  try {
    entries = readdirSync(fileURLToPath(COMMANDS_DIR))
  } catch (error) {
    warn(`stupid-comments: could not read command definitions: ${String(error)}`)
    return []
  }

  const commands = []
  for (const entry of entries) {
    if (!entry.endsWith('.md')) continue
    const parsed = parseCommandFile(new URL(entry, COMMANDS_DIR))
    if (parsed) commands.push({ slug: entry.slice(0, -3), ...parsed })
  }
  return commands
}

function parseCommandFile(url) {
  let raw
  try {
    raw = readFileSync(fileURLToPath(url), 'utf8')
  } catch {
    return undefined
  }

  const { frontmatter, body } = splitFrontmatter(raw)
  const description = frontmatter.description ?? 'Comment policy command.'
  if (!body.trim()) return undefined
  return {
    description,
    ...frontmatter['argument-hint'] ? { hint: frontmatter['argument-hint'] } : {},
    body: body.trim(),
  }
}

function splitFrontmatter(raw) {
  const text = raw.replace(/^\uFEFF/, '')
  if (!text.startsWith('---')) return { frontmatter: {}, body: text }
  const end = text.indexOf('\n---', 3)
  if (end === -1) return { frontmatter: {}, body: text }

  const frontmatter = {}
  for (const line of text.slice(3, end).split('\n')) {
    const at = line.indexOf(':')
    if (at === -1) continue
    frontmatter[line.slice(0, at).trim()] = line.slice(at + 1).trim()
  }
  const bodyStart = text.indexOf('\n', end + 1)
  return { frontmatter, body: bodyStart === -1 ? '' : text.slice(bodyStart + 1) }
}

/** The `$1`, `${1:-default}`, and `$ARGUMENTS` placeholders Claude Code expands. */
export function expand(body, rawInput) {
  const input = rawInput.trim()
  const args = input ? input.split(/\s+/) : []
  return body
    .replace(/\$\{(\d+):-([^}]*)\}/g, (_, index, fallback) => args[Number(index) - 1] ?? fallback)
    .replace(/\$\{(\d+)\}/g, (_, index) => args[Number(index) - 1] ?? '')
    .replace(/\$ARGUMENTS\b/g, input)
    .replace(/\$(\d+)/g, (_, index) => args[Number(index) - 1] ?? '')
}
