#!/usr/bin/env node
// Protocol smoke test for the same debug adapter used by the editors.
'use strict';

const assert = require('node:assert/strict');
const { spawn } = require('node:child_process');
const { mkdtempSync, rmSync, writeFileSync } = require('node:fs');
const { tmpdir } = require('node:os');
const path = require('node:path');
const { setTimeout: delay } = require('node:timers/promises');

const bin = process.argv[2] || process.env.AEVIA_CLI || 'aevia';
const fixture = mkdtempSync(path.join(tmpdir(), 'aevia debugger '));
const program = path.join(fixture, 'motion.aev');
writeFileSync(program, `# A debugger session starts here.
fn increment(x: Int): Int = x + 1

fn main(): IO Unit = do
    let initial = { count = 40, distances = [1.0 m, 2.0 m] }
    let total = increment(initial.count)
    println(total)
`);
const child = spawn(bin, ['debug', '--stdio'], { stdio: ['pipe', 'pipe', 'inherit'] });
const messages = [];
const closed = new Promise(resolve => child.once('close', (code, signal) => resolve([code, signal])));
let processError;
child.on('error', error => { processError = error; });
child.stdin.on('error', error => { processError = error; });
let buffer = Buffer.alloc(0);
let sequence = 0;

child.stdout.on('data', chunk => {
  buffer = Buffer.concat([buffer, chunk]);
  for (;;) {
    const separator = buffer.indexOf('\r\n\r\n');
    if (separator < 0) return;
    const length = Number(/Content-Length: (\d+)/i.exec(buffer.subarray(0, separator).toString())[1]);
    const end = separator + 4 + length;
    if (buffer.length < end) return;
    messages.push(JSON.parse(buffer.subarray(separator + 4, end).toString()));
    buffer = buffer.subarray(end);
  }
});

async function take(predicate, description) {
  const deadline = Date.now() + 20_000;
  while (Date.now() < deadline) {
    const index = messages.findIndex(predicate);
    if (index >= 0) return messages.splice(index, 1)[0];
    if (processError) throw processError;
    if (child.exitCode !== null || child.signalCode !== null) {
      throw new Error(`Adapter exited before ${description}: ${JSON.stringify(messages)}`);
    }
    await delay(20);
  }
  throw new Error(`Timed out waiting for ${description}: ${JSON.stringify(messages)}`);
}

async function request(command, args = {}) {
  const seq = ++sequence;
  const body = Buffer.from(JSON.stringify({ seq, type: 'request', command, arguments: args }));
  child.stdin.write(Buffer.concat([Buffer.from(`Content-Length: ${body.length}\r\n\r\n`), body]));
  const reply = await take(message => message.type === 'response' && message.request_seq === seq, command);
  assert.equal(reply.success, true, JSON.stringify(reply));
  return reply.body;
}

function event(name, filter = () => true) {
  return take(message => message.type === 'event' && message.event === name && filter(message.body), name);
}

async function main() {
  const capabilities = await request('initialize', { clientID: 'aevia-smoke', linesStartAt1: true, columnsStartAt1: true });
  assert.equal(capabilities.supportsConfigurationDoneRequest, true);
  await request('launch', { program });
  await event('initialized');
  const pending = await request('setBreakpoints', { source: { path: program }, breakpoints: [{ line: 6 }, { line: 1 }] });
  assert.equal(pending.breakpoints[0].verified, false);
  await request('configurationDone');
  const verified = await event('breakpoint', body => body.breakpoint.line === 6);
  assert.equal(verified.body.breakpoint.verified, true);
  const comment = await event('breakpoint', body => body.breakpoint.line === 1);
  assert.equal(comment.body.breakpoint.verified, false);
  const stopped = await event('stopped');
  assert.equal(stopped.body.reason, 'breakpoint');
  assert.deepEqual(stopped.body.hitBreakpointIds, [pending.breakpoints[0].id]);
  const stack = await request('stackTrace', { threadId: 1, levels: 1 });
  assert.equal(stack.stackFrames[0].line, 6);
  const scopes = await request('scopes', { frameId: stack.stackFrames[0].id });
  const locals = await request('variables', { variablesReference: scopes.scopes[0].variablesReference });
  const initial = locals.variables.find(variable => variable.name === 'initial');
  assert.ok(initial.variablesReference > 0);
  const fields = await request('variables', { variablesReference: initial.variablesReference });
  assert.equal(fields.variables.find(variable => variable.name === 'count').value, '40');
  const distances = fields.variables.find(variable => variable.name === 'distances');
  const elements = await request('variables', { variablesReference: distances.variablesReference });
  assert.match(elements.variables[0].value, /^1(\.0)? m$/);

  await request('stepIn', { threadId: 1 });
  const stepped = await event('stopped');
  assert.equal(stepped.body.reason, 'step');
  const inFunction = await request('stackTrace', { threadId: 1 });
  assert.equal(inFunction.stackFrames[0].line, 2);
  assert.match(inFunction.stackFrames[0].name, /increment/);
  const arg = await request('evaluate', { frameId: inFunction.stackFrames[0].id, expression: 'x' });
  assert.equal(arg.result, '40');
  await request('stepOut', { threadId: 1 });
  assert.equal((await event('stopped')).body.reason, 'step');
  const leftFunction = await request('stackTrace', { threadId: 1 });
  assert.equal(leftFunction.stackFrames[0].line, 7);
  await request('continue', { threadId: 1 });
  const printed = await event('output', body => body.category === 'stdout');
  assert.equal(printed.body.output, '41\n');
  const exited = await event('exited');
  assert.equal(exited.body.exitCode, 0);
  await event('terminated');
  await request('disconnect');
  child.stdin.end();
  assert.deepEqual(await closed, [0, null]);
  console.log('Aevia DAP smoke passed');
}

main().catch(error => {
  console.error(error);
  child.kill();
  process.exitCode = 1;
}).finally(() => rmSync(fixture, { recursive: true, force: true }));
