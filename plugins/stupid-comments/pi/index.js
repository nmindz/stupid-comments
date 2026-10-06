// Extension for Pi (earendil-works/pi) and its oh-my-pi fork (omp, omp-web).
// The Rust binary holds every rule; this turns their tool calls and stop
// seams into the hook payload it already speaks, and its verdict back into a
// block or a continuation.

import { readFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { resolve } from 'node:path'
import { expand, loadCommands } from '../lib/commands.js'
import { createRunner, disarmed, PLUGIN } from '../lib/engine.js'

const CUSTOM_TYPE = PLUGIN

export default function stupidComments(pi) {
  apply(pi)
}

export function apply(pi, config = {}) {
  if (disarmed()) return

  const run = createRunner({ client: 'pi', binary: config.binary, timeoutMs: config.timeoutMs })
  const options = (ctx) => ({ cwd: ctx.cwd, signal: ctx.signal, warn: notifier(ctx) })

  // A throwing tool_call handler blocks the tool, so nothing here may throw.
  pi.on('tool_call', async (event, ctx) => {
    try {
      const call = normalize(event, ctx.cwd)
      if (!call) return undefined
      const outcome = await run(preToolPayload(event, call, ctx), options(ctx))
      if (outcome.block) return { block: true, reason: outcome.message }
      if (outcome.message) pi.sendMessage(finding(outcome.message), { triggerTurn: false })
    } catch {}
    return undefined
  })

  // omp's stop seam speaks Claude Code's Stop hook and tracks
  // `stop_hook_active` itself. Pi never fires it.
  pi.on('session_stop', async (event, ctx) => {
    try {
      const outcome = await run(stopPayload(ctx, event.stop_hook_active === true), options(ctx))
      if (!outcome.message) return undefined
      if (!outcome.block) {
        pi.sendMessage(finding(outcome.message), { triggerTurn: false })
        return undefined
      }
      // omp hides the continuation it sends, so say why the run goes on.
      notifier(ctx)(`${PLUGIN}: comment policy violations left behind; sending the model back to rewrite them.`)
      return { decision: 'block', reason: outcome.message }
    } catch {
      return undefined
    }
  })

  // Pi's equivalent: one forced continuation per prompt, then the stop after it
  // reports `stop_hook_active` so a stubborn violation cannot loop the run.
  // omp never fires these.
  let forced = false
  pi.on('agent_settled', () => { forced = false })
  pi.on('agent_before_settle', async (event, ctx) => {
    if (event.outcome && event.outcome !== 'completed') return undefined
    try {
      const outcome = await run(stopPayload(ctx, forced), options(ctx))
      if (!outcome.message) return undefined
      const entries = [...event.entries ?? [], { type: 'custom_message', ...finding(outcome.message) }]
      if (!outcome.block) return { entries }
      forced = true
      return { entries, continue: true }
    } catch {
      return undefined
    }
  })

  for (const command of loadCommands()) {
    pi.registerCommand(`${PLUGIN}:${command.slug}`, {
      description: command.description,
      handler: async (args, ctx) => {
        const text = expand(command.body, args ?? '')
        pi.sendUserMessage(text, ctx.isIdle() ? undefined : { deliverAs: 'steer' })
      },
    })
  }
}

function finding(text) {
  return { customType: CUSTOM_TYPE, content: text, display: true }
}

function notifier(ctx) {
  return (message) => {
    if (ctx.hasUI) ctx.ui.notify(message, 'warning')
    else console.error(message)
  }
}

// --- Payloads. Field names are the Claude Code hook input schema, because the
// binary parses one dialect and every harness feeds it. ---

function base(ctx, event) {
  const session = ctx.sessionManager
  return {
    session_id: session?.getSessionId?.() ?? '',
    transcript_path: session?.getSessionFile?.() ?? '',
    cwd: ctx.cwd ?? process.cwd(),
    hook_event_name: event,
  }
}

function preToolPayload(event, call, ctx) {
  return {
    ...base(ctx, 'PreToolUse'),
    tool_name: call.tool,
    tool_input: call.input,
    tool_use_id: event.toolCallId,
  }
}

function stopPayload(ctx, active) {
  return { ...base(ctx, 'Stop'), stop_hook_active: active }
}

/**
 * Restate `write` and `edit` in the engine's dialect. Both hosts name the file
 * `path` and resolve it against the session cwd. Pi spells anchors
 * `edits[].oldText/newText`; omp's `replace` mode already uses the engine's
 * `old_string`/`new_string`, and its hashline and patch modes carry no anchor
 * to rebuild from, so they fall to the stop gate.
 */
function normalize(event, cwd) {
  const input = event.input ?? {}
  if (typeof input.path !== 'string' || !input.path) return undefined
  const file = absolute(input.path, cwd)

  switch (event.toolName?.toLowerCase()) {
    case 'write':
      if (typeof input.content !== 'string') return undefined
      return { tool: 'write', input: { file_path: file, content: input.content } }
    case 'edit': {
      if (typeof input.old_string === 'string' && typeof input.new_string === 'string') {
        const { old_string, new_string, replace_all } = input
        return { tool: 'edit', input: { file_path: file, old_string, new_string, replace_all: replace_all === true } }
      }
      const edits = Array.isArray(input.edits) ? input.edits : []
      if (!edits.length || !edits.every(e => typeof e?.oldText === 'string' && typeof e?.newText === 'string')) {
        return undefined
      }
      return {
        tool: 'edit',
        input: { file_path: file, edits: bottomUp(file, edits).map(e => ({ old_string: e.oldText, new_string: e.newText })) },
      }
    }
    default:
      return undefined
  }
}

/**
 * Pi matches every anchor against the original file, while the engine applies
 * edits in sequence. Applied bottom-up, no edit can disturb the text an
 * earlier anchor is found in, so both readings produce the same file.
 */
function bottomUp(file, edits) {
  let original
  try {
    original = readFileSync(file, 'utf8')
  } catch {
    return edits
  }
  return edits
    .map(edit => ({ edit, at: original.indexOf(edit.oldText) }))
    .sort((a, b) => b.at - a.at)
    .map(({ edit }) => edit)
}

/** Pi's own path rules: a leading `@` is dropped and `~` is the home directory. */
function absolute(path, cwd) {
  const bare = path.startsWith('@') ? path.slice(1) : path
  if (bare === '~') return homedir()
  if (bare.startsWith('~/')) return resolve(homedir(), bare.slice(2))
  return resolve(cwd ?? process.cwd(), bare)
}
