// Functional test for the DSH adapter. It drives the real binary through a
// fake cordis context, so a green run means the seams, the payload dialect,
// and the exit-code mapping all still agree with each other.
//
// Run: node plugins/stupid-comments/dsh/test.mjs [path-to-binary]

import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { mkdtempSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repo = resolve(fileURLToPath(new URL('../../../', import.meta.url)))
const binary = process.argv[2] ?? join(repo, 'target/release/stupid-comments')

const project = mkdtempSync(join(tmpdir(), 'stupid-comments-dsh-'))
writeFileSync(join(project, 'AGENTS.md'), '# Comments Policy\n\nComments earn their place or they go.\n')
writeFileSync(
  join(project, '.stupid-comments.jsonc'),
  '{ "mode": "block", "bannedPatterns": ["obviously stupid"] }\n',
)

// Isolate policy discovery from the machine running the test.
process.env.HOME = project
process.env.CLAUDE_CONFIG_DIR = join(project, 'absent-claude')
process.env.DSH_HOME = join(project, 'absent-dsh')
process.env.AGENTS_HOME = join(project, 'absent-agents')

const { apply } = await import('./index.js')

const ALLOWED = { kind: 'allowed-by-next' }

function createContext() {
  const listeners = new Map()
  const commands = []
  const warnings = []
  const ctx = {
    logger: { warn: (message) => warnings.push(message) },
    on: (event, handler) => { listeners.set(event, handler) },
    get: () => undefined,
    inject: (_deps, callback) => { callback(ctx) },
    commands: { register: (definition) => { commands.push(definition) } },
  }
  return { ctx, listeners, commands, warnings }
}

function createAgent() {
  const steered = []
  const injected = []
  return {
    steered,
    injected,
    agent: {
      session: { header: { id: 'test-session', cwd: project } },
      steer: (message) => steered.push(message.content[0].text),
      inject: (message) => injected.push(message.content[0].text),
    },
  }
}

/** The subagent seam is detached, so wait for its effect instead of sleeping. */
async function until(condition, timeoutMs = 15_000) {
  const deadline = Date.now() + timeoutMs
  while (!condition()) {
    if (Date.now() > deadline) throw new Error('timed out waiting for the detached run')
    await new Promise(resolve => setTimeout(resolve, 20))
  }
}

function writeCall(agent, content) {
  return {
    name: 'write',
    callId: 'call-1',
    agent,
    arguments: { file_path: join(project, 'sample.ts'), content },
    signal: new AbortController().signal,
  }
}

const VIOLATION = '// this is an obviously stupid comment\nexport const x = 1\n'
const CLEAN = 'export const x = 1\n'

// --- A violating write is denied, and the model is told why. ---
{
  const { ctx, listeners, commands } = createContext()
  apply(ctx, { binary })
  const { agent } = createAgent()

  const decision = await listeners.get('tools/pre-execute')(writeCall(agent, VIOLATION), () => ALLOWED)
  assert.equal(decision.kind, 'deny', 'a banned comment must be denied')
  assert.match(decision.reason, /banned-pattern/, 'the denial carries the failing rule')
  assert.match(decision.reason, /Comments earn their place/, 'the denial re-injects the policy verbatim')

  assert.equal(commands.length, 4, 'every command markdown file registers')
  for (const command of commands) {
    assert.match(command.name, /^stupid-comments-[a-z0-9_-]+$/, `${command.name} is a legal DSH command name`)
    assert.ok(command.description.length > 0, `${command.name} carries a description`)
  }
}

// --- A clean write delegates to the rest of the pipeline. ---
{
  const { ctx, listeners } = createContext()
  apply(ctx, { binary })
  const { agent } = createAgent()

  const decision = await listeners.get('tools/pre-execute')(writeCall(agent, CLEAN), () => ALLOWED)
  assert.equal(decision, ALLOWED, 'a clean write reaches next()')
}

// --- The text-editor tool dialect is translated, not ignored. ---
{
  const { ctx, listeners } = createContext()
  apply(ctx, { binary })
  const { agent } = createAgent()
  const signal = new AbortController().signal

  const created = {
    name: 'str_replace_editor',
    callId: 'call-2',
    agent,
    signal,
    arguments: { command: 'create', path: join(project, 'sample.ts'), file_text: VIOLATION },
  }
  const decision = await listeners.get('tools/pre-execute')(created, () => ALLOWED)
  assert.equal(decision.kind, 'deny', 'a create through the text editor is checked like a write')

  // A view carries no content and must not reach the binary at all.
  const viewed = { ...created, arguments: { command: 'view', path: join(project, 'sample.ts') } }
  assert.equal(await listeners.get('tools/pre-execute')(viewed, () => ALLOWED), ALLOWED)
}

// --- An unwatched tool never spawns the binary. ---
{
  const { ctx, listeners } = createContext()
  apply(ctx, { binary: join(project, 'no-such-binary') })
  const { agent } = createAgent()

  const exec = { ...writeCall(agent, VIOLATION), name: 'read' }
  assert.equal(await listeners.get('tools/pre-execute')(exec, () => ALLOWED), ALLOWED)
}

// --- A missing binary warns once and enforces nothing. ---
{
  const { ctx, listeners, warnings } = createContext()
  apply(ctx, { binary: join(project, 'no-such-binary') })
  const { agent } = createAgent()

  assert.equal(await listeners.get('tools/pre-execute')(writeCall(agent, VIOLATION), () => ALLOWED), ALLOWED)
  assert.equal(await listeners.get('tools/pre-execute')(writeCall(agent, VIOLATION), () => ALLOWED), ALLOWED)
  assert.equal(warnings.length, 1, 'the missing binary is reported once, not per call')
  assert.match(warnings[0], /cargo install/)
}

// --- A command steers the shared prompt with its arguments expanded. ---
{
  const { ctx, commands } = createContext()
  apply(ctx, { binary })
  const { agent, steered } = createAgent()

  const check = commands.find(c => c.name === 'stupid-comments-check')
  assert.ok(check, 'the check command is registered')
  assert.equal(check.input.hint, '[path]', 'the argument hint survives the frontmatter')

  const result = check.handler({ agent, rawInput: '', attachments: [], signal: new AbortController().signal })
  assert.equal(result.kind, 'success')
  assert.match(steered[0], /stupid-comments check \./, 'an omitted argument falls back to the default')

  check.handler({ agent, rawInput: ' crates ', attachments: [], signal: new AbortController().signal })
  assert.match(steered[1], /stupid-comments check crates/, 'a supplied argument is substituted')
}

// --- A violating file left behind at the end of a turn forces continuation. ---
{
  execFileSync('git', ['init', '--quiet'], { cwd: project })
  writeFileSync(join(project, 'left-behind.ts'), VIOLATION)

  const { ctx, listeners } = createContext()
  apply(ctx, { binary })
  const { agent, steered } = createAgent()

  await listeners.get('agent/turn-stopping')({ agent, turn: 1, signal: new AbortController().signal })
  assert.equal(steered.length, 1, 'the stopping turn is steered back to the violation')
  assert.match(steered[0], /banned-pattern/)
}

// --- A subagent that ends on a violation is told what it left behind. ---
{
  const { ctx, listeners } = createContext()
  const child = createAgent()
  ctx.get = (service) => (service === 'agents' ? { get: () => child.agent } : undefined)
  apply(ctx, { binary })

  listeners.get('subagent/start')({ id: 'child', runId: 'run-1' })
  await listeners.get('subagent/end')({ id: 'child', runId: 'run-1' })
  await until(() => child.injected.length === 1)
  assert.equal(child.injected.length, 1, 'the child is handed the finding as context')
  assert.match(child.injected[0], /banned-pattern/)
}

// --- A subagent whose handle was never seen is skipped, not crashed on. ---
{
  const { ctx, listeners } = createContext()
  apply(ctx, { binary })
  await listeners.get('subagent/end')({ id: 'child', runId: 'run-1' })
}

// --- The environment disarm switch turns the whole adapter off. ---
{
  process.env.STUPID_COMMENTS = '0'
  const { ctx, listeners, commands } = createContext()
  apply(ctx, { binary })
  assert.equal(listeners.size, 0, 'no seams are registered while disarmed')
  assert.equal(commands.length, 0, 'no commands are registered while disarmed')
  delete process.env.STUPID_COMMENTS
}

console.log('dsh plugin: all checks passed')
