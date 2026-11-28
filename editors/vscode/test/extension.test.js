'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const test = require('node:test');
const vm = require('node:vm');
const { fileURLToPath, pathToFileURL } = require('node:url');

function editor(fixture) {
  const settings = new Map([['cli.path', process.execPath]]);
  const commands = new Map();
  const watchers = [];
  const languageClients = [];
  const tasks = [];
  const terminals = [];
  const messages = [];
  const context = { subscriptions: [] };

  const vscode = {
    workspace: {
      textDocuments: [],
      getWorkspaceFolder: () => undefined,
      getConfiguration: () => ({ get: (key, fallback) => settings.get(key) ?? fallback }),
      onDidChangeConfiguration: () => ({ dispose() {} }),
      createFileSystemWatcher(pattern) {
        const watcher = { pattern, disposed: false, dispose() { this.disposed = true; } };
        watchers.push(watcher);
        return watcher;
      },
    },
    window: {
      showErrorMessage(text) { messages.push(text); },
      showInformationMessage(text) { messages.push(text); },
      createTerminal(options) {
        terminals.push(options);
        return { show() {} };
      },
    },
    debug: {
      registerDebugConfigurationProvider: () => ({ dispose() {} }),
      registerDebugAdapterDescriptorFactory: () => ({ dispose() {} }),
    },
    commands: {
      registerCommand(name, handler) {
        assert.equal(commands.has(name), false, `duplicate command ${name}`);
        commands.set(name, handler);
        return { dispose() { commands.delete(name); } };
      },
    },
    Uri: {
      parse(uri) {
        const url = new URL(uri);
        return { scheme: url.protocol.slice(0, -1), fsPath: fileURLToPath(url), toString: () => uri };
      },
    },
    Task: class {
      constructor(definition, scope, name, source, execution) {
        Object.assign(this, { definition, scope, name, source, execution });
      }
    },
    ProcessExecution: class {
      constructor(executable, args, options) { Object.assign(this, { executable, args, options }); }
    },
    TaskScope: { Global: 1, Workspace: 2 },
    TaskRevealKind: { Always: 1 },
    TaskPanelKind: { Dedicated: 2 },
    tasks: {
      async executeTask(task) { tasks.push(task); },
    },
  };

  class LanguageClient {
    constructor(_id, _name, _server, options) {
      this.options = options;
      languageClients.push(this);
    }
    async start() {
      this.registrations = ['aevia.runFile', 'aevia.testFile'].map(command =>
        vscode.commands.registerCommand(command, (...args) => this.options.middleware.executeCommand(command, args)),
      );
    }
    async dispose() {
      this.registrations.forEach(command => command.dispose());
    }
  }

  settings.set('server.path', process.execPath);
  const module = { exports: {} };
  const source = fs.readFileSync(path.join(__dirname, '../extension.js'), 'utf8');
  vm.runInNewContext(source, {
    require(name) {
      if (name === 'vscode') return vscode;
      if (name === 'vscode-languageclient/node') return { LanguageClient, TransportKind: { stdio: 0 } };
      return require(name);
    },
    process,
    module,
  });

  function open(file, props = {}) {
    const uri = vscode.Uri.parse(pathToFileURL(path.join(fixture, file)).href);
    const document = { uri, languageId: 'aevia', isDirty: false, ...props };
    vscode.workspace.textDocuments.push(document);
    return document;
  }

  return { open, vscode, commands, watchers, languageClients, tasks, terminals, messages, context, ...module.exports };
}

test('a test lens runs its own document and test while menu commands use focus', async t => {
  const fixture = fs.mkdtempSync(path.join(os.tmpdir(), 'aevia extension '));
  const vscode = editor(fixture);
  t.after(async () => {
    await vscode.deactivate();
    fs.rmSync(fixture, { recursive: true, force: true });
  });
  const source = path.join(fixture, 'src');
  fs.mkdirSync(source);
  fs.writeFileSync(path.join(fixture, 'aevia.toml'), '');
  const left = vscode.open('src/left.aev');
  const right = vscode.open('src/right space.aev');
  vscode.vscode.window.activeTextEditor = { document: left };
  await vscode.activate(vscode.context);

  await vscode.commands.get('aevia.testFile')(right.uri.toString(), 'orbit tests');
  assert.deepEqual([...vscode.tasks[0].execution.args], ['test', right.uri.fsPath, '--exact=orbit tests']);
  assert.equal(vscode.tasks[0].execution.options.cwd, fixture);
  await vscode.commands.get('aevia.runCurrentFile')();
  assert.deepEqual([...vscode.tasks[1].execution.args], ['run', left.uri.fsPath]);
  await vscode.commands.get('aevia.openRepl')();
  assert.deepEqual([...vscode.terminals[0].shellArgs], ['repl']);
  assert.equal(vscode.terminals[0].cwd, fixture);

  await Promise.all([vscode.commands.get('aevia.restartServer')(), vscode.commands.get('aevia.restartServer')()]);
  assert.equal(vscode.languageClients.length, 3);
  assert.equal(vscode.watchers.filter(watcher => !watcher.disposed).length, 3);
});

test('a failed save prevents execution', async t => {
  const vscode = editor(os.tmpdir());
  t.after(() => vscode.deactivate());
  const document = vscode.open('unsaved.aev', { isDirty: true, async save() { return false; } });
  vscode.vscode.window.activeTextEditor = { document };
  await vscode.activate(vscode.context);
  await vscode.commands.get('aevia.runCurrentFile')();
  assert.equal(vscode.tasks.length, 0);
  assert.match(vscode.messages[0], /could not be saved/);
});
