// DSH (DeepSeek Harness) adapter. The Rust binary holds every rule; this turns
// harness seams into the hook payload it already speaks, and its exit code back
// into a DSH decision, so both harnesses run identical logic.
//
// Every failure path is silent: a missing binary never blocks a write.

import { spawn } from 'node:child_process'
import { randomUUID } from 'node:crypto'
import { readdirSync, readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

export const name = 'stupid-comments'

const DISARM_ENV = 'STUPID_COMMENTS'
const DEFAULT_BINARY = 'stupid-comments'
const DEFAULT_TIMEOUT_MS = 15_000
const BLOCK_EXIT_CODE = 2
const SOURCE = { kind: 'plugin', plugin: name }

/** Tools whose arguments carry file content the policy applies to. */
const WATCHED_TOOLS = new Set(['write', 'edit', 'multiedit', 'multi_edit', 'str_replace_editor'])

const COMMANDS_DIR = new URL('../commands/', import.meta.url)
const COMMAND_PREFIX = 'stupid-comments-'

export function apply(ctx, config = {}) {
  if (process.env[DISARM_ENV] === '0') return

  const binary = config.binary ?? DEFAULT_BINARY
  const timeoutMs = config.timeoutMs ?? DEFAULT_TIMEOUT_MS
  const run = createRunner(ctx, binary, timeoutMs)

  ctx.on('tools/pre-execute', async (exec, next) => {
    if (!WATCHED_TOOLS.has(exec.name.toLowerCase())) return next()
    const call = normalize(exec)
    if (!call) return next()
    const outcome = await run(exec.agent, preToolPayload(exec, call), exec.signal)
    if (outcome.block) return { kind: 'deny', reason: outcome.message }
    if (outcome.message) inject(exec.agent, outcome.message)
    return next()
  })

  // A blocking Stop steers the agent instead of letting it settle, which is
  // how Claude Code's Stop hook forces the model to fix what it just wrote.
  ctx.on('agent/turn-stopping', async ({ agent, signal }) => {
    const outcome = await run(agent, stopPayload(agent), signal)
    if (outcome.block) steer(agent, outcome.message)
    else if (outcome.message) inject(agent, outcome.message)
  })

  // A child is retained from its start edge: by the time it ends, the registry
  // may already have dropped it, and without the handle there is no workspace
  // to run the check in.
  const children = new Map()
  ctx.on('subagent/start', (info) => {
    const child = ctx.get('agents')?.get(info.id)
    if (child) children.set(info.runId ?? info.id, child)
  })
  ctx.on('subagent/end', (info) => {
    const key = info.runId ?? info.id
    const child = children.get(key) ?? ctx.get('agents')?.get(info.id)
    children.delete(key)
    if (!child) return
    void run(child, subagentStopPayload(child, info)).then((outcome) => {
      if (outcome.message) inject(child, outcome.message)
    })
  })

  ctx.inject(['commands'], (commandCtx) => {
    for (const command of loadCommands(ctx)) {
      commandCtx.commands.register({
        name: COMMAND_PREFIX + command.slug,
        description: command.description,
        ...command.hint ? { input: { hint: command.hint } } : {},
        handler: (invocation) => {
          steer(invocation.agent, expand(command.body, invocation.rawInput))
          return { kind: 'success', text: `Running ${command.slug} against the comment policy.` }
        },
      })
    }
  })
}

/**
 * One spawn of `stupid-comments hook dsh` with the payload on stdin. The first
 * ENOENT disables the runner for the rest of the process: a user who installed
 * the plugin but not the binary gets one warning, not one per tool call.
 */
function createRunner(ctx, binary, timeoutMs) {
  const quiet = { block: false, message: '' }
  let missing = false

  return async function run(agent, payload, signal) {
    if (missing || process.env[DISARM_ENV] === '0') return quiet
    const cwd = workspaceOf(agent)

    try {
      const result = await execute(binary, payload, { cwd, timeoutMs, signal })
      const message = result.stderr.trim()
      if (!message) return quiet
      return { block: result.code === BLOCK_EXIT_CODE, message }
    } catch (error) {
      if (error?.code === 'ENOENT') {
        missing = true
        ctx.logger?.warn(
          `${name}: "${binary}" is not on PATH, so nothing is being enforced. `
          + 'Install it with: cargo install --root ~/.local --git https://github.com/nmindz/stupid-comments stupid-comments',
        )
      }
      return quiet
    }
  }
}

function execute(binary, payload, { cwd, timeoutMs, signal }) {
  return new Promise((resolve, reject) => {
    const child = spawn(binary, ['hook', 'dsh'], {
      ...cwd ? { cwd } : {},
      stdio: ['pipe', 'ignore', 'pipe'],
    })

    let stderr = ''
    let settled = false
    const finish = (fn, value) => {
      if (settled) return
      settled = true
      clearTimeout(timer)
      signal?.removeEventListener('abort', abort)
      fn(value)
    }
    const abort = () => {
      child.kill('SIGKILL')
      finish(resolve, { code: 0, stderr: '' })
    }
    const timer = setTimeout(abort, timeoutMs)

    child.stderr.setEncoding('utf8')
    child.stderr.on('data', (chunk) => { stderr += chunk })
    child.on('error', (error) => { finish(reject, error) })
    child.on('close', (code) => { finish(resolve, { code: code ?? 0, stderr }) })
    signal?.addEventListener('abort', abort, { once: true })

    child.stdin.on('error', () => {})
    child.stdin.end(JSON.stringify(payload))
  })
}

// --- Payloads. Field names are the Claude Code hook input schema, because the
// binary parses one dialect and both harnesses feed it. ---

function base(agent, event) {
  return {
    session_id: agent?.session?.header?.id ?? agent?.session?.id ?? '',
    transcript_path: '',
    cwd: workspaceOf(agent) ?? process.cwd(),
    hook_event_name: event,
  }
}

function preToolPayload(exec, call) {
  return {
    ...base(exec.agent, 'PreToolUse'),
    tool_name: call.tool,
    tool_input: call.input,
    tool_use_id: exec.callId,
  }
}

/**
 * Restate a tool call in the two operations the engine understands. Anthropic's
 * text-editor tool names the same file, content, and anchors differently, and
 * translating here keeps that dialect out of the engine. An `insert` command
 * carries no anchor to reconstruct from, so it falls to the stop gate.
 */
function normalize(exec) {
  const args = exec.arguments ?? {}
  if (exec.name.toLowerCase() !== 'str_replace_editor') {
    return { tool: exec.name, input: args }
  }
  if (args.command === 'create') {
    return { tool: 'write', input: { file_path: args.path, content: args.file_text } }
  }
  if (args.command === 'str_replace') {
    return { tool: 'edit', input: { file_path: args.path, old_string: args.old_str, new_string: args.new_str } }
  }
  return undefined
}

function stopPayload(agent) {
  return { ...base(agent, 'Stop'), stop_hook_active: false }
}

function subagentStopPayload(agent, info) {
  return { ...base(agent, 'SubagentStop'), agent_id: info.id, stop_hook_active: false }
}

function workspaceOf(agent) {
  return agent?.session?.header?.cwd ?? undefined
}

// --- Model-facing messages. Built inline so the plugin stays dependency-free
// and installable into any profile. ---

function userMessage(text) {
  return deepFreeze({
    id: randomUUID(),
    role: 'user',
    content: [{ type: 'text', text }],
    source: SOURCE,
  })
}

function deepFreeze(value) {
  for (const key of Object.getOwnPropertyNames(value)) {
    const child = value[key]
    if (child && typeof child === 'object') deepFreeze(child)
  }
  return Object.freeze(value)
}

function steer(agent, text) {
  try {
    agent?.steer(userMessage(text))
  } catch {}
}

function inject(agent, text) {
  try {
    agent?.inject(userMessage(text))
  } catch {}
}

// --- Commands. The prompt bodies are the same markdown files Claude Code
// loads, so neither harness owns a private copy of the wording. ---

function loadCommands(ctx) {
  let entries
  try {
    entries = readdirSync(fileURLToPath(COMMANDS_DIR))
  } catch (error) {
    ctx.logger?.warn(`${name}: could not read command definitions: ${String(error)}`)
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
function expand(body, rawInput) {
  const input = rawInput.trim()
  const args = input ? input.split(/\s+/) : []
  return body
    .replace(/\$\{(\d+):-([^}]*)\}/g, (_, index, fallback) => args[Number(index) - 1] ?? fallback)
    .replace(/\$\{(\d+)\}/g, (_, index) => args[Number(index) - 1] ?? '')
    .replace(/\$ARGUMENTS\b/g, input)
    .replace(/\$(\d+)/g, (_, index) => args[Number(index) - 1] ?? '')
}
