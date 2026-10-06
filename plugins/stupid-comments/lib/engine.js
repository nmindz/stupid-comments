// Transport shared by the harness adapters: one spawn of the binary per check,
// the hook payload on stdin, the exit code back as a verdict.
//
// Every failure path is silent: a missing binary never blocks a write.

import { spawn } from 'node:child_process'

export const PLUGIN = 'stupid-comments'
export const DISARM_ENV = 'STUPID_COMMENTS'
export const DEFAULT_BINARY = 'stupid-comments'
export const DEFAULT_TIMEOUT_MS = 15_000

const BLOCK_EXIT_CODE = 2
const INSTALL_HINT = 'cargo install --root ~/.local --git https://github.com/nmindz/stupid-comments stupid-comments'
const QUIET = Object.freeze({ block: false, message: '' })

export function disarmed() {
  return process.env[DISARM_ENV] === '0'
}

/**
 * A `run(payload, { cwd, signal, warn })` that spawns `stupid-comments hook
 * <client>`. The first unusable binary disables the runner for the rest of the
 * process: a user whose binary is missing or too old gets one warning, not one
 * per tool call.
 */
export function createRunner({ client, binary: requested, timeoutMs: limit, warn = () => {} }) {
  const binary = requested ?? DEFAULT_BINARY
  const timeoutMs = limit ?? DEFAULT_TIMEOUT_MS
  let unusable = false
  let handshake

  return async function run(payload, { cwd, signal, warn: report = warn } = {}) {
    if (unusable || disarmed()) return QUIET
    handshake ??= probe(report)
    if (!await handshake) {
      unusable = true
      return QUIET
    }

    try {
      const result = await execute(binary, client, payload, { cwd, timeoutMs, signal })
      const message = result.stderr.trim()
      if (!message) return QUIET
      return { block: result.code === BLOCK_EXIT_CODE, message }
    } catch {
      return QUIET
    }
  }

  /**
   * Ask the binary to answer an empty payload before trusting its exit codes.
   * A binary older than this client rejects the argument through its argument
   * parser, which exits 2 — the same code that means "block this write".
   * Without this handshake a stale install denies every write with a usage
   * error as the reason.
   */
  async function probe(report) {
    try {
      const result = await execute(binary, client, {}, { timeoutMs })
      if (result.code === 0) return true
      report(
        `${PLUGIN}: "${binary}" does not understand this plugin (\`hook ${client}\` exited ${result.code}), `
        + `so nothing is being enforced. Update it with: ${INSTALL_HINT}`,
      )
    } catch (error) {
      const reason = error?.code === 'ENOENT' ? 'is not on PATH' : `could not be run (${String(error)})`
      report(`${PLUGIN}: "${binary}" ${reason}, so nothing is being enforced. Install it with: ${INSTALL_HINT}`)
    }
    return false
  }
}

function execute(binary, client, payload, { cwd, timeoutMs, signal }) {
  return new Promise((resolve, reject) => {
    const child = spawn(binary, ['hook', client], {
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
