#!/usr/bin/env node
// Drives `aevia-lsp --stdio` the way an editor does and checks that each
// feature the plugins rely on answers. Usage: node lsp-smoke.js [path-to-aevia-lsp]
'use strict';
const { spawn } = require('child_process');
const { mkdtempSync, rmSync, writeFileSync } = require('fs');
const { tmpdir } = require('os');
const path = require('path');
const { pathToFileURL } = require('url');

const bin = process.argv[2] || process.env.AEVIA_LSP || 'aevia-lsp';
const fixture = mkdtempSync(path.join(tmpdir(), 'aevia lsp '));
const file = path.join(fixture, 'smoke.aev');
const text = 'let distance = 3.2 km\nfn main(): IO Unit = println("editor run complete")\n\n'
  + 'test "selection" = false\n\ntest "exact selection" = true\n';
writeFileSync(file, text);
const uri = pathToFileURL(file).href;

const child = spawn(bin, ['--stdio'], { stdio: ['pipe', 'pipe', 'inherit'] });
const closed = new Promise(resolve => child.once('close', resolve));
child.once('close', () => rmSync(fixture, { force: true, recursive: true }));
const timeout = setTimeout(() => {
  console.error('Aevia language server smoke timed out');
  child.kill();
  process.exitCode = 1;
}, 30_000);
child.on('error', error => {
  console.error(error.message);
  clearTimeout(timeout);
  process.exitCode = 1;
});
let buf = Buffer.alloc(0);
const pending = new Map();
const notes = [];
let nextId = 1;

child.stdout.on('data', (d) => {
  buf = Buffer.concat([buf, d]);
  for (;;) {
    const sep = buf.indexOf('\r\n\r\n');
    if (sep < 0) return;
    const m = /Content-Length: (\d+)/i.exec(buf.slice(0, sep).toString());
    const len = Number(m[1]);
    if (buf.length < sep + 4 + len) return;
    const msg = JSON.parse(buf.slice(sep + 4, sep + 4 + len).toString('utf8'));
    buf = buf.slice(sep + 4 + len);
    if (msg.id !== undefined && pending.has(msg.id)) {
      const resolve = pending.get(msg.id);
      pending.delete(msg.id);
      resolve(msg);
    } else notes.push(msg);
  }
});

function send(obj) {
  const body = Buffer.from(JSON.stringify(obj), 'utf8');
  child.stdin.write(`Content-Length: ${body.length}\r\n\r\n`);
  child.stdin.write(body);
}
function request(method, params) {
  const id = nextId++;
  return new Promise((resolve) => {
    pending.set(id, resolve);
    send({ jsonrpc: '2.0', id, method, params });
  });
}
const notify = (method, params) => send({ jsonrpc: '2.0', method, params });

let failed = 0;
function check(name, ok, detail) {
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? '  ' + detail : ''}`);
  if (!ok) failed++;
}

(async () => {
  const init = await request('initialize', {
    processId: process.pid,
    rootUri: pathToFileURL(path.resolve(__dirname, '../..')).href,
    capabilities: {},
  });
  const caps = init.result && init.result.capabilities;
  check('initialize', !!caps);
  notify('initialized', {});
  notify('textDocument/didOpen', { textDocument: { uri, languageId: 'aevia', version: 1, text } });

  const td = { uri };
  const hover = await request('textDocument/hover', { textDocument: td, position: { line: 0, character: 4 } });
  check('hover', typeof hover.result?.contents?.value === 'string', JSON.stringify(hover.result || null).slice(0, 80));
  const sym = await request('textDocument/documentSymbol', { textDocument: td });
  check('documentSymbol', Array.isArray(sym.result) && sym.result.length > 0);
  const comp = await request('textDocument/completion', { textDocument: td, position: { line: 1, character: 21 } });
  check('completion', Array.isArray(comp.result) && comp.result.length > 0);
  const sem = await request('textDocument/semanticTokens/full', { textDocument: td });
  check('semanticTokens', sem.result && sem.result.data && sem.result.data.length > 0);
  const fmt = await request('textDocument/formatting', { textDocument: td, options: { tabSize: 4, insertSpaces: true } });
  check('formatting', fmt.error === undefined);

  const lenses = await request('textDocument/codeLens', { textDocument: td });
  const commands = (lenses.result || []).map(lens => lens.command);
  const run = commands.find(command => command.command === 'aevia.runFile');
  const selected = commands.find(command =>
    command.command === 'aevia.testFile' && command.arguments[1] === 'exact selection');
  const failing = commands.find(command =>
    command.command === 'aevia.testFile' && command.arguments[1] === 'selection');
  check('run and test code lenses', commands.length === 3 && [run, selected, failing]
    .every(command => command?.arguments[0] === uri), JSON.stringify(commands));

  if (run && selected && failing) {
    notify('textDocument/didChange', {
      textDocument: { uri, version: 2 },
      contentChanges: [{ text: text.replace('editor run complete', 'unsaved replacement') }],
    });
    const unsaved = await request('workspace/executeCommand', run);
    check('unsaved source refuses to run', unsaved.error?.code === -32803
      && unsaved.error.message.includes('save the Aevia file'), JSON.stringify(unsaved));
    notify('textDocument/didChange', {
      textDocument: { uri, version: 3 }, contentChanges: [{ text }],
    });
    const ran = await request('workspace/executeCommand', run);
    check('run captures the real CLI output', ran.result?.success && ran.result.exitCode === 0
      && ran.result.stdout.trim() === 'editor run complete'
      && !ran.result.timedOut && !ran.result.stdoutTruncated, JSON.stringify(ran.result || ran.error));
    check('run publishes output to the editor log', notes.some(note =>
      note.method === 'window/logMessage' && note.params.message.includes('editor run complete')));
    const passed = await request('workspace/executeCommand', selected);
    check('test code lens selects one exact name', passed.result?.success
      && passed.result.stdout.includes('1 passed, 0 failed'), JSON.stringify(passed.result || passed.error));
    const result = await request('workspace/executeCommand', failing);
    check('failing test is reported to the editor', result.result?.success === false
      && result.result.exitCode === 1 && result.result.stdout.includes('0 passed, 1 failed'),
      JSON.stringify(result.result || result.error));
  }

  // a deliberate type error must come back as a diagnostic
  const bad = pathToFileURL(path.resolve(__dirname, 'bad.aev')).href;
  notify('textDocument/didOpen', {
    textDocument: { uri: bad, languageId: 'aevia', version: 1, text: 'fn main(): IO Unit = do\n    let x = 1 m + 2 s\n    println("{x}")\n' },
  });
  for (let i = 0; i < 50 && !notes.some((n) => n.method === 'textDocument/publishDiagnostics' && n.params.uri === bad); i++)
    await new Promise((r) => setTimeout(r, 100));
  const diag = notes.find((n) => n.method === 'textDocument/publishDiagnostics' && n.params.uri === bad);
  check('diagnostics', diag && diag.params.diagnostics.length > 0, diag && diag.params.diagnostics[0] && diag.params.diagnostics[0].message);

  await request('shutdown', null);
  notify('exit', null);
  child.stdin.end();
  check('shutdown', (await closed) === 0);
  clearTimeout(timeout);
  process.exitCode = failed ? 1 : 0;
})().catch(error => {
  console.error(error);
  child.kill();
  clearTimeout(timeout);
  process.exitCode = 1;
});
