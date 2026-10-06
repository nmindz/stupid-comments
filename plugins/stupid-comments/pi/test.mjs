// Functional test for the Pi extension. It drives the real binary through a
// fake ExtensionAPI, so a green run means the events, the payload dialect, and
// the exit-code mapping all still agree with each other.
//
// Run: node plugins/stupid-comments/pi/test.mjs [path-to-binary]

import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { chmodSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repo = resolve(fileURLToPath(new URL('../../../', import.meta.url)))
const binary = process.argv[2] ?? join(repo, 'target/release/stupid-comments')

const root = mkdtempSync(join(tmpdir(), 'stupid-comments-pi-'))
const project = join(root, 'block')
const lenient = join(root, 'warn')
for (const [dir, mode] of [[project, 'block'], [lenient, 'warn']]) {
  mkdirSync(dir)
  writeFileSync(join(dir, 'AGENTS.md'), '# Comments Policy\n\nComments earn their place or they go.\n')
  writeFileSync(join(dir, '.stupid-comments.jsonc'), `{ "mode": "${mode}", "bannedPatterns": ["obviously stupid"] }\n`)
}

// Isolate policy discovery from the machine running the test.
process.env.HOME = root
process.env.CLAUDE_CONFIG_DIR = join(root, 'absent-claude')
process.env.DSH_HOME = join(root, 'absent-dsh')
process.env.PI_CODING_AGENT_DIR = join(root, 'absent-pi')
process.env.AGENTS_HOME = join(root, 'absent-agents')

const { default: extension, apply } = await import('./index.js')

function createPi() {
  const handlers = new Map()
  const commands = new Map()
  const sent = []
  const prompts = []
  const pi = {
    on: (event, handler) => {
      if (!handlers.has(event)) handlers.set(event, [])
      handlers.get(event).push(handler)
      return () => {}
    },
    registerCommand: (name, options) => { commands.set(name, options) },
    sendMessage: (message, options) => { sent.push({ message, options }) },
    sendUserMessage: (content, options) => { prompts.push({ content, options }) },
  }
  /** Chains handlers the way Pi's runner does: the last returned value wins. */
  const emit = async (event, payload, ctx) => {
    let result
    for (const handler of handlers.get(event) ?? []) result = await handler(payload, ctx) ?? result
    return result
  }
  return { pi, handlers, commands, sent, prompts, emit }
}

function createContext({ cwd = project, idle = true, hasUI = true } = {}) {
  const notices = []
  return {
    notices,
    ctx: {
      cwd,
      hasUI,
      ui: { notify: (message, level) => { notices.push({ message, level }) } },
      signal: new AbortController().signal,
      sessionManager: { getSessionId: () => 'pi-test-session', getSessionFile: () => undefined },
      isIdle: () => idle,
    },
  }
}

function toolCall(toolName, input) {
  return { type: 'tool_call', toolCallId: 'call-1', toolName, input }
}

function settle(outcome = 'completed', entries = []) {
  return { type: 'agent_before_settle', outcome, entries, continue: false }
}

const VIOLATION = '// this is an obviously stupid comment\nexport const x = 1\n'
const CLEAN = 'export const x = 1\n'

// --- The default export is the factory Pi calls. ---
assert.equal(typeof extension, 'function', 'Pi loads a default-exported factory')

// --- A violating write is blocked, and the model is told why. ---
{
  const { pi, emit, commands } = createPi()
  apply(pi, { binary })
  const { ctx } = createContext()

  const decision = await emit('tool_call', toolCall('write', { path: join(project, 'sample.ts'), content: VIOLATION }), ctx)
  assert.equal(decision?.block, true, 'a banned comment must be blocked')
  assert.match(decision.reason, /banned-pattern/, 'the block carries the failing rule')
  assert.match(decision.reason, /Comments earn their place/, 'the block re-injects the policy verbatim')

  assert.equal(commands.size, 4, 'every command markdown file registers')
  for (const [name, command] of commands) {
    assert.match(name, /^stupid-comments:[a-z0-9_-]+$/, `${name} matches the Claude Code spelling`)
    assert.ok(command.description.length > 0, `${name} carries a description`)
  }
}

// --- A clean write passes untouched. ---
{
  const { pi, emit, sent } = createPi()
  apply(pi, { binary })
  const { ctx } = createContext()

  assert.equal(await emit('tool_call', toolCall('write', { path: join(project, 'clean.ts'), content: CLEAN }), ctx), undefined)
  assert.equal(sent.length, 0, 'a clean write leaves nothing behind')
}

// --- Stripping a file bare in the same session is noticed, through Pi's session id. ---
{
  const { pi, emit, sent } = createPi()
  apply(pi, { binary })
  const { ctx } = createContext()
  const write = (content) => emit('tool_call', toolCall('write', { path: join(project, 'stripped.ts'), content }), ctx)

  assert.equal(await write('// Rounded to cents before display.\nexport const x = 1\n'), undefined)
  assert.equal(sent.length, 0, 'a legitimate comment draws nothing')
  const decision = await write(CLEAN)
  assert.equal(decision, undefined, 'the evasion signal warns rather than blocks')
  assert.match(sent[0]?.message.content ?? '', /comments-removed/, 'the earlier comment count was remembered')
}

// --- Paths resolve the way Pi resolves them, against the session cwd. ---
{
  const { pi, emit } = createPi()
  apply(pi, { binary })
  const { ctx } = createContext()

  for (const path of ['sample.ts', '@sample.ts', './nested/../sample.ts']) {
    const decision = await emit('tool_call', toolCall('write', { path, content: VIOLATION }), ctx)
    assert.equal(decision?.block, true, `${path} resolves against the session cwd`)
  }
}

// --- Pi's edit dialect is translated, and only the lines it writes answer. ---
{
  const { pi, emit } = createPi()
  apply(pi, { binary })
  const { ctx } = createContext()
  const file = join(project, 'edited.ts')
  const edit = (edits) => emit('tool_call', toolCall('edit', { path: file, edits }), ctx)

  writeFileSync(file, CLEAN)
  const introduced = await edit([{ oldText: CLEAN, newText: VIOLATION }])
  assert.equal(introduced?.block, true, 'an edit that writes a banned comment is blocked')

  writeFileSync(file, 'export const a = 1\nexport const b = 2\n// this is an obviously stupid comment\nexport const c = 3\n')
  const beside = await edit([{ oldText: 'export const c = 3', newText: 'export const c = 4' }])
  assert.equal(beside, undefined, 'an edit is not blamed for the violation beside it')

  // Pi matches each anchor against the original file, in any order. The second
  // edit grows the file above the first.
  const reordered = await edit([
    { oldText: 'export const c = 3', newText: 'export const c = 4' },
    { oldText: 'export const a = 1\n', newText: 'export const a = 1\nexport const z = 0\n' },
  ])
  assert.equal(reordered, undefined, 'an out-of-order multi-edit still answers only for its own lines')

  const missing = await edit([{ oldText: 'no such anchor', newText: VIOLATION }])
  assert.equal(missing, undefined, 'an anchor that is absent stands the gate down')

  // Pi matches the second anchor against the original line 3. Applied
  // top-down in sequence it would hit the first edit's new text instead and
  // hide the violation that edit wrote.
  writeFileSync(file, 'export const a = 1\nexport const b = 2\n// this is an obviously stupid comment\nexport const c = 3\n')
  const topDown = await edit([
    { oldText: 'export const a = 1', newText: '// obviously stupid comment here\nexport const a = 1' },
    { oldText: 'obviously stupid comment', newText: 'clear comment' },
  ])
  assert.equal(topDown?.block, true, 'edits in Pi\'s order still reconstruct the file Pi will write')
  assert.match(topDown.reason, /:1-1 {2}\[banned-pattern\]/, 'the violation the first edit wrote is the one reported')

  writeFileSync(file, 'export const a = 1\nexport const b = 2\n// this is an obviously stupid comment\nexport const c = 3\n')
  const deleted = await edit([{ oldText: 'export const b = 2\n', newText: '' }])
  assert.equal(deleted, undefined, 'deleting a line does not blame the one that slides into its place')
}

// --- omp's edit modes: `replace` is checked, hashline falls to the stop gate. ---
{
  const { pi, emit } = createPi()
  apply(pi, { binary })
  const { ctx } = createContext()
  const file = join(project, 'omp-edited.ts')
  writeFileSync(file, CLEAN)

  const replace = await emit('tool_call', toolCall('edit', { path: file, old_string: CLEAN, new_string: VIOLATION }), ctx)
  assert.equal(replace?.block, true, 'an omp replace-mode edit that writes a banned comment is blocked')

  // omp derives `path` from the hashline header; the payload carries no anchors.
  const hashline = { input: `@@ ${file}#a1b2\n+// this is an obviously stupid comment\n`, path: file, paths: [file] }
  assert.equal(await emit('tool_call', toolCall('edit', hashline), ctx), undefined, 'a hashline edit is left to the stop gate')
}

// --- omp's session_stop seam blocks with the Claude Code decision shape. ---
{
  const { pi, emit, sent } = createPi()
  apply(pi, { binary })
  const { ctx, notices } = createContext()
  const stop = (active) => ({ type: 'session_stop', session_id: 's', messages: [], turn_id: 1, stop_hook_active: active, signal: new AbortController().signal })

  writeFileSync(join(project, 'omp-left-behind.ts'), VIOLATION)
  execFileSync('git', ['init', '--quiet'], { cwd: project })
  const result = await emit('session_stop', stop(false), ctx)
  assert.equal(result?.decision, 'block', 'omp is asked to continue')
  assert.match(result.reason, /omp-left-behind\.ts.*banned-pattern/s, 'the reason is the model-visible finding')
  assert.equal(notices.length, 1, 'the user is told why the run goes on, since omp hides the reason')

  assert.equal(await emit('session_stop', stop(true), ctx), undefined, 'omp\'s own stop_hook_active ends the loop')
  assert.equal(sent.length, 0)
}

{
  const { pi, emit, sent } = createPi()
  apply(pi, { binary })
  const { ctx } = createContext({ cwd: lenient })
  execFileSync('git', ['init', '--quiet'], { cwd: lenient })
  writeFileSync(join(lenient, 'omp-left-behind.ts'), VIOLATION)

  const stop = { type: 'session_stop', session_id: 's', messages: [], turn_id: 1, stop_hook_active: false, signal: new AbortController().signal }
  assert.equal(await emit('session_stop', stop, ctx), undefined, 'a warning does not continue the run')
  assert.equal(sent.length, 1, 'the warning is recorded for the model to see')
  assert.equal(sent[0].options.triggerTurn, false)
}

// --- A warn-level finding reaches the model as context, without blocking. ---
{
  const { pi, emit, sent } = createPi()
  apply(pi, { binary })
  const { ctx } = createContext({ cwd: lenient })

  const decision = await emit('tool_call', toolCall('write', { path: join(lenient, 'sample.ts'), content: VIOLATION }), ctx)
  assert.equal(decision, undefined, 'a warning never blocks')
  assert.equal(sent.length, 1, 'the warning is sent to the session')
  assert.equal(sent[0].message.customType, 'stupid-comments')
  assert.match(sent[0].message.content, /banned-pattern/)
  assert.equal(sent[0].options.triggerTurn, false, 'a warning does not start a turn of its own')
}

// --- An unwatched tool never spawns the binary. ---
{
  const { pi, emit } = createPi()
  apply(pi, { binary: join(root, 'no-such-binary') })
  const { ctx, notices } = createContext()

  for (const name of ['read', 'bash', 'grep']) {
    assert.equal(await emit('tool_call', toolCall(name, { path: join(project, 'sample.ts'), content: VIOLATION }), ctx), undefined)
  }
  assert.equal(notices.length, 0, 'nothing was run, so nothing was reported')
}

// --- A missing binary warns once and enforces nothing. ---
{
  const { pi, emit } = createPi()
  apply(pi, { binary: join(root, 'no-such-binary') })
  const { ctx, notices } = createContext()

  const write = toolCall('write', { path: join(project, 'sample.ts'), content: VIOLATION })
  assert.equal(await emit('tool_call', write, ctx), undefined)
  assert.equal(await emit('tool_call', write, ctx), undefined)
  assert.equal(notices.length, 1, 'the missing binary is reported once, not per call')
  assert.equal(notices[0].level, 'warning')
  assert.match(notices[0].message, /cargo install/)
}

// --- A binary too old to know this client must not read as a block. ---
{
  const stale = join(root, 'stale-binary')
  writeFileSync(stale, '#!/bin/sh\necho "error: invalid value \'pi\' for \'<CLIENT>\'" >&2\nexit 2\n')
  chmodSync(stale, 0o755)

  const { pi, emit } = createPi()
  apply(pi, { binary: stale })
  const { ctx, notices } = createContext()

  const write = toolCall('write', { path: join(project, 'sample.ts'), content: VIOLATION })
  assert.equal(await emit('tool_call', write, ctx), undefined)
  assert.equal(await emit('tool_call', write, ctx), undefined)
  assert.equal(notices.length, 1, 'the stale binary is reported once, not per call')
  assert.match(notices[0].message, /does not understand this plugin/)
}

// --- A command sends the shared prompt with its arguments expanded. ---
{
  const { pi, commands, prompts } = createPi()
  apply(pi, { binary })

  const check = commands.get('stupid-comments:check')
  assert.ok(check, 'the check command is registered')

  await check.handler('', createContext().ctx)
  assert.match(prompts[0].content, /stupid-comments check \./, 'an omitted argument falls back to the default')
  assert.equal(prompts[0].options, undefined, 'an idle session takes the prompt as a new turn')

  await check.handler(' crates ', createContext({ idle: false }).ctx)
  assert.match(prompts[1].content, /stupid-comments check crates/, 'a supplied argument is substituted')
  assert.deepEqual(prompts[1].options, { deliverAs: 'steer' }, 'a busy session is steered rather than refused')
}

// --- A violating file left behind at settle forces one continuation per prompt. ---
{
  execFileSync('git', ['init', '--quiet'], { cwd: project })
  writeFileSync(join(project, 'left-behind.ts'), VIOLATION)

  const { pi, emit } = createPi()
  apply(pi, { binary })
  const { ctx } = createContext()

  const prior = { type: 'custom', customType: 'another-extension' }
  const first = await emit('agent_before_settle', settle('completed', [prior]), ctx)
  assert.equal(first?.continue, true, 'the settling run is sent back to the violation')
  assert.equal(first.entries.length, 2, 'entries proposed by earlier handlers are kept')
  assert.equal(first.entries[0], prior)
  assert.equal(first.entries[1].type, 'custom_message')
  assert.equal(first.entries[1].customType, 'stupid-comments')
  assert.equal(first.entries[1].display, true)
  assert.match(first.entries[1].content, /banned-pattern/)

  // Claude Code sets stop_hook_active on the stop that follows a forced
  // continuation; without it a stubborn violation would loop the run forever.
  assert.equal(await emit('agent_before_settle', settle(), ctx), undefined, 'a prompt is forced to continue at most once')

  await emit('agent_settled', { type: 'agent_settled' }, ctx)
  const next = await emit('agent_before_settle', settle(), ctx)
  assert.equal(next?.continue, true, 'the next prompt is gated afresh')

  await emit('agent_settled', { type: 'agent_settled' }, ctx)
  for (const outcome of ['aborted', 'error']) {
    assert.equal(await emit('agent_before_settle', settle(outcome), ctx), undefined, `an ${outcome} run is left to settle`)
  }
}

// --- A warn-level finding at settle is recorded without forcing a continuation. ---
{
  execFileSync('git', ['init', '--quiet'], { cwd: lenient })
  writeFileSync(join(lenient, 'left-behind.ts'), VIOLATION)

  const { pi, emit } = createPi()
  apply(pi, { binary })
  const { ctx } = createContext({ cwd: lenient })

  const result = await emit('agent_before_settle', settle(), ctx)
  assert.equal(result?.entries?.length, 1, 'the warning is recorded in the session')
  assert.equal(result.continue, undefined, 'a warning leaves the continuation decision to others')
}

// --- The environment disarm switch turns the whole extension off. ---
{
  process.env.STUPID_COMMENTS = '0'
  const { pi, handlers, commands } = createPi()
  apply(pi, { binary })
  assert.equal(handlers.size, 0, 'no events are handled while disarmed')
  assert.equal(commands.size, 0, 'no commands are registered while disarmed')
  delete process.env.STUPID_COMMENTS
}

console.log('pi extension: all checks passed')
