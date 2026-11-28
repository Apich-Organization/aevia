'use strict';

const vscode = require('vscode');
const fs = require('node:fs');
const path = require('node:path');
const { LanguageClient, TransportKind } = require('vscode-languageclient/node');

const runLens = 'aevia.runFile';
const testLens = 'aevia.testFile';
let client;
let serverWork = Promise.resolve();

function executable(file) {
  try {
    fs.accessSync(file, fs.constants.X_OK);
    return fs.statSync(file).isFile();
  } catch {
    return false;
  }
}

function onPath(name) {
  const filename = process.platform === 'win32' ? name + '.exe' : name;
  for (const directory of (process.env.PATH || '').split(path.delimiter)) {
    if (!path.isAbsolute(directory)) continue;
    const candidate = path.join(directory, filename);
    if (executable(candidate)) return candidate;
  }
  return undefined;
}

function resolveServer() {
  const config = vscode.workspace.getConfiguration('aevia');
  const extra = config.get('server.args', []);
  const configured = config.get('server.path', '').trim();
  if (configured) return { command: configured, args: ['--stdio', ...extra] };
  const lsp = onPath('aevia-lsp');
  if (lsp) return { command: lsp, args: ['--stdio', ...extra] };
  const cli = resolveCli();
  if (cli) return { command: cli, args: ['lsp', '--stdio', ...extra] };
  return undefined;
}

async function start() {
  const server = resolveServer();
  if (!server) {
    const pick = await vscode.window.showErrorMessage(
      'Aevia language server unavailable. Install aevia-lsp or the aevia CLI.',
      'Open Settings',
    );
    if (pick) await vscode.commands.executeCommand('workbench.action.openSettings', 'aevia.server.path');
    return;
  }
  const serverOptions = { ...server, transport: TransportKind.stdio };
  const fileEvents = ['**/*.aev', '**/aevia.toml', '**/aevia.lock']
    .map(pattern => vscode.workspace.createFileSystemWatcher(pattern));
  const next = new LanguageClient('aevia', 'Aevia Language Server', serverOptions, {
    documentSelector: [
      { scheme: 'file', language: 'aevia' },
      { scheme: 'vscode-remote', language: 'aevia' },
      { scheme: 'untitled', language: 'aevia' },
    ],
    synchronize: { fileEvents },
    middleware: {
      executeCommand(command, args, forward) {
        if (command === runLens) return runFile('run', ...args);
        if (command === testLens) return runFile('test', ...args);
        return forward(command, args);
      },
    },
  });
  client = { next, fileEvents };
  try {
    await next.start();
  } catch (error) {
    await stop();
    vscode.window.showErrorMessage(`Aevia language server failed to start: ${error.message}`);
  }
}

async function stop() {
  if (!client) return;
  const { next, fileEvents } = client;
  client = undefined;
  try {
    await next.dispose();
  } finally {
    for (const watcher of fileEvents) watcher.dispose();
  }
}

function restart() {
  serverWork = serverWork.catch(() => {}).then(async () => {
    await stop();
    await start();
  });
  return serverWork;
}

function resolveCli() {
  const config = vscode.workspace.getConfiguration('aevia');
  const configured = config.get('cli.path', '').trim();
  if (configured) return configured;
  const cli = onPath('aevia');
  if (cli) return cli;
  const serverPath = config.get('server.path', '').trim() || onPath('aevia-lsp');
  if (serverPath && path.isAbsolute(serverPath)) {
    const ext = process.platform === 'win32' ? '.exe' : '';
    const sibling = path.join(path.dirname(serverPath), 'aevia' + ext);
    if (executable(sibling)) return sibling;
  }
  return undefined;
}

async function cliForExecution() {
  const cli = resolveCli();
  if (cli) return cli;
  const pick = await vscode.window.showErrorMessage(
    'Aevia CLI unavailable. Install aevia or configure its path.',
    'Open Settings',
  );
  if (pick) await vscode.commands.executeCommand('workbench.action.openSettings', 'aevia.cli.path');
  return undefined;
}

function isSavedFile(document) {
  return document && ['file', 'vscode-remote'].includes(document.uri.scheme);
}

function documentFor(uri) {
  if (typeof uri !== 'string') return vscode.window.activeTextEditor?.document;
  const target = vscode.Uri.parse(uri);
  return vscode.workspace.textDocuments.find(document => {
    return document.uri.toString() === target.toString()
      || (isSavedFile(document) && document.uri.fsPath === target.fsPath);
  });
}

function workingDirectory(file) {
  const containing = path.dirname(file);
  let directory = containing;
  while (true) {
    try {
      if (fs.statSync(path.join(directory, 'aevia.toml')).isFile()) return directory;
    } catch { /* move to the parent */ }
    const parent = path.dirname(directory);
    if (parent === directory) return containing;
    directory = parent;
  }
}

async function saveDocument(document) {
  if (!isSavedFile(document)) {
    vscode.window.showInformationMessage('Aevia: save the source file first.');
    return false;
  }
  if (!document.isDirty || await document.save()) return true;
  vscode.window.showErrorMessage('Aevia: the source file could not be saved.');
  return false;
}

async function runFile(subcommand, uri, filter) {
  const document = documentFor(uri);
  if (document?.languageId !== 'aevia') {
    vscode.window.showInformationMessage('Aevia: open an .aev file to run or test.');
    return;
  }
  if (!await saveDocument(document)) return;
  const cli = await cliForExecution();
  if (!cli) return;
  const file = document.uri.fsPath;
  const args = [subcommand, file];
  if (subcommand === 'test' && typeof filter === 'string') args.push(`--exact=${filter}`);
  const name = subcommand === 'test' ? `Test ${filter || path.basename(file)}` : `Run ${path.basename(file)}`;
  const folder = vscode.workspace.getWorkspaceFolder(document.uri);
  const task = new vscode.Task(
    { type: 'aevia', subcommand, file, filter },
    folder || vscode.TaskScope.Global,
    name,
    'aevia',
    new vscode.ProcessExecution(cli, args, { cwd: workingDirectory(file) }),
    '$aevia',
  );
  task.presentationOptions = { reveal: vscode.TaskRevealKind.Always, panel: vscode.TaskPanelKind.Dedicated };
  await vscode.tasks.executeTask(task);
}

async function openRepl() {
  const cli = await cliForExecution();
  if (!cli) return;
  const document = vscode.window.activeTextEditor?.document;
  const cwd = document?.languageId === 'aevia' && isSavedFile(document)
    ? workingDirectory(document.uri.fsPath)
    : vscode.workspace.workspaceFolders?.[0]?.uri;
  vscode.window.createTerminal({ name: 'Aevia REPL', shellPath: cli, shellArgs: ['repl'], cwd }).show();
}

function launchConfiguration() {
  const document = vscode.window.activeTextEditor?.document;
  if (document?.languageId !== 'aevia') return undefined;
  return { type: 'aevia', name: 'Debug Aevia File', request: 'launch', program: '${file}', args: [] };
}

function registerDebugger(context) {
  context.subscriptions.push(
    vscode.commands.registerCommand('aevia.debugFile', async () => {
      const config = launchConfiguration();
      const document = vscode.window.activeTextEditor?.document;
      if (!config || !await saveDocument(document)) return;
      await vscode.debug.startDebugging(vscode.workspace.getWorkspaceFolder(document.uri), config);
    }),
    vscode.debug.registerDebugConfigurationProvider('aevia', {
      provideDebugConfigurations() {
        const config = launchConfiguration();
        return config ? [config] : [];
      },
      async resolveDebugConfiguration(folder, config) {
        if (!config.type && !config.request && !config.name) config = launchConfiguration();
        if (!config?.program) return undefined;
        const document = vscode.window.activeTextEditor?.document;
        if (config.program === '${file}' && !await saveDocument(document)) return undefined;
        return config;
      },
      async resolveDebugConfigurationWithSubstitutedVariables(folder, config) {
        const document = vscode.workspace.textDocuments.find(doc => doc.uri.fsPath === config.program);
        if (document?.isDirty && !await saveDocument(document)) return undefined;
        return config;
      },
    }),
    vscode.debug.registerDebugAdapterDescriptorFactory('aevia', {
      async createDebugAdapterDescriptor() {
        const cli = await cliForExecution();
        if (!cli) return undefined;
        return new vscode.DebugAdapterExecutable(cli, ['debug', '--stdio']);
      },
    }),
  );
}

async function activate(context) {
  context.subscriptions.push(
    vscode.commands.registerCommand('aevia.runCurrentFile', () => runFile('run')),
    vscode.commands.registerCommand('aevia.testCurrentFile', () => runFile('test')),
    vscode.commands.registerCommand('aevia.openRepl', openRepl),
    vscode.commands.registerCommand('aevia.restartServer', restart),
    vscode.commands.registerCommand('aevia.showServerLog', () => client?.next.outputChannel.show()),
    vscode.workspace.onDidChangeConfiguration(event => {
      if (event.affectsConfiguration('aevia.server') || event.affectsConfiguration('aevia.cli.path')) void restart();
    }),
  );
  registerDebugger(context);
  await restart();
}

async function deactivate() {
  await serverWork.catch(() => {});
  await stop();
}

module.exports = { activate, deactivate };
