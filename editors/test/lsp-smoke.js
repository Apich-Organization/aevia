#!/usr/bin/env node
// Drives `aevia-lsp --stdio` the way an editor does and checks that each
// feature the plugins rely on answers. Usage: node lsp-smoke.js [path-to-aevia-lsp]
'use strict';
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const { pathToFileURL } = require('url');

const bin = process.argv[2] || process.env.AEVIA_LSP || 'aevia-lsp';
const file = path.resolve(__dirname, '../../examples/units.aev');
const text = fs.readFileSync(file, 'utf8');
const uri = pathToFileURL(file).href;

const child = spawn(bin, ['--stdio'], { stdio: ['pipe', 'pipe', 'inherit'] });
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
    if (msg.id !== undefined && pending.has(msg.id)) pending.get(msg.id)(msg);
    else notes.push(msg);
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
  const hover = await request('textDocument/hover', { textDocument: td, position: { line: 4, character: 8 } });
  check('hover', hover.error === undefined, JSON.stringify(hover.result || null).slice(0, 80));
  const sym = await request('textDocument/documentSymbol', { textDocument: td });
  check('documentSymbol', Array.isArray(sym.result) && sym.result.length > 0);
  const comp = await request('textDocument/completion', { textDocument: td, position: { line: 4, character: 8 } });
  check('completion', comp.error === undefined);
  const sem = await request('textDocument/semanticTokens/full', { textDocument: td });
  check('semanticTokens', sem.result && sem.result.data && sem.result.data.length > 0);
  const fmt = await request('textDocument/formatting', { textDocument: td, options: { tabSize: 4, insertSpaces: true } });
  check('formatting', fmt.error === undefined);

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
  setTimeout(() => process.exit(failed ? 1 : 0), 200);
})();
