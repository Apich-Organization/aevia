// Aevia for VS Code: a thin launcher for `aevia-lsp`.
// Everything the editor shows (diagnostics, hover, completion, rename,
// formatting, semantic tokens) comes from the server.
'use strict';

const vscode = require('vscode');
const fs = require('fs');
const path = require('path');
const { LanguageClient, TransportKind } = require('vscode-languageclient/node');

/** @type {LanguageClient | undefined} */
let client;

function onPath(name) {
  const exts = process.platform === 'win32' ? ['.exe', '.cmd', '.bat', ''] : [''];
  for (const dir of (process.env.PATH || '').split(path.delimiter)) {
    for (const ext of exts) {
      const candidate = path.join(dir, name + ext);
      try {
        if (fs.statSync(candidate).isFile()) return candidate;
      } catch (_) { /* keep looking */ }
    }
  }
  return undefined;
}

/** Resolves the server command: setting, then `aevia-lsp`, then `aevia lsp`. */
function resolveServer() {
  const cfg = vscode.workspace.getConfiguration('aevia');
  const extra = cfg.get('server.args', []);
  const configured = (cfg.get('server.path', '') || '').trim();
  if (configured) return { command: configured, args: ['--stdio', ...extra] };
  const lsp = onPath('aevia-lsp');
  if (lsp) return { command: lsp, args: ['--stdio', ...extra] };
  const cli = onPath('aevia');
  if (cli) return { command: cli, args: ['lsp', '--stdio', ...extra] };
  return undefined;
}

async function start(context) {
  const server = resolveServer();
  if (!server) {
    const pick = await vscode.window.showErrorMessage(
      'Aevia: neither `aevia-lsp` nor `aevia` was found on PATH.',
      'Set aevia.server.path');
    if (pick) vscode.commands.executeCommand('workbench.action.openSettings', 'aevia.server.path');
    return;
  }
  const serverOptions = {
    run: { command: server.command, args: server.args, transport: TransportKind.stdio },
    debug: { command: server.command, args: server.args, transport: TransportKind.stdio },
  };
  const clientOptions = {
    documentSelector: [{ scheme: 'file', language: 'aevia' }, { scheme: 'untitled', language: 'aevia' }],
    synchronize: {
      fileEvents: [
        vscode.workspace.createFileSystemWatcher('**/*.aev'),
        vscode.workspace.createFileSystemWatcher('**/aevia.toml'),
      ],
    },
  };
  client = new LanguageClient('aevia', 'Aevia Language Server', serverOptions, clientOptions);
  context.subscriptions.push({ dispose: () => client && client.stop() });
  await client.start();
}

async function stop() {
  if (client) {
    const c = client;
    client = undefined;
    await c.stop();
  }
}

/** Resolves the `aevia` CLI executable: `aevia.cli.path`, PATH, or sibling of `aevia.server.path`. */
function resolveCli() {
  const cfg = vscode.workspace.getConfiguration('aevia');
  const configured = (cfg.get('cli.path', '') || '').trim();
  if (configured) return configured;
  const cli = onPath('aevia');
  if (cli) return cli;
  const serverPath = (cfg.get('server.path', '') || '').trim();
  if (serverPath) {
    const ext = process.platform === 'win32' ? '.exe' : '';
    const sibling = path.join(path.dirname(serverPath), 'aevia' + ext);
    try {
      if (fs.statSync(sibling).isFile()) return sibling;
    } catch (_) { /* ignore */ }
  }
  return undefined;
}

/** Runs `aevia <subcommand> <file>` in a VS Code task terminal. */
async function runAeviaSubcommand(subcommand) {
  const editor = vscode.window.activeTextEditor;
  if (!editor || editor.document.languageId !== 'aevia') {
    vscode.window.showInformationMessage('Aevia: open an .aev file to run or test.');
    return;
  }
  if (editor.document.isUntitled) {
    vscode.window.showErrorMessage('Aevia: save the file before running.');
    return;
  }
  if (editor.document.isDirty) {
    await editor.document.save();
  }
  const cli = resolveCli();
  if (!cli) {
    const pick = await vscode.window.showErrorMessage(
      'Aevia: `aevia` CLI was not found on PATH.',
      'Set aevia.cli.path',
    );
    if (pick) vscode.commands.executeCommand('workbench.action.openSettings', 'aevia.cli.path');
    return;
  }
  const filePath = editor.document.uri.fsPath;
  const folder = vscode.workspace.getWorkspaceFolder(editor.document.uri);
  const cwd = folder ? folder.uri.fsPath : path.dirname(filePath);
  const label = `${subcommand === 'test' ? 'Test' : 'Run'} ${path.basename(filePath)}`;
  const execution = new vscode.ProcessExecution(cli, [subcommand, filePath], { cwd });
  const task = new vscode.Task(
    { type: 'aevia', command: subcommand },
    folder || vscode.TaskScope.Workspace,
    label,
    'aevia',
    execution,
  );
  task.presentationOptions = {
    reveal: vscode.TaskRevealKind.Always,
    panel: vscode.TaskPanelKind.Shared,
    clear: false,
  };
  await vscode.tasks.executeTask(task);
}

async function activate(context) {
  context.subscriptions.push(
    vscode.commands.registerCommand('aevia.runFile', () => runAeviaSubcommand('run')),
    vscode.commands.registerCommand('aevia.testFile', () => runAeviaSubcommand('test')),
    vscode.commands.registerCommand('aevia.restartServer', async () => {
      await stop();
      await start(context);
    }),
    vscode.commands.registerCommand('aevia.showServerLog', () => client && client.outputChannel.show()),
    vscode.workspace.onDidChangeConfiguration(async (e) => {
      if (e.affectsConfiguration('aevia.server')) {
        await stop();
        await start(context);
      }
    }),
  );
  await start(context);
}

module.exports = { activate, deactivate: stop };
