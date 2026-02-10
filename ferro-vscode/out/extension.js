"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.activate = activate;
exports.deactivate = deactivate;
const path = require("path");
const vscode = require("vscode");
const node_1 = require("vscode-languageclient/node");
let client;
function activate(context) {
    // Get the compiler path from settings
    const config = vscode.workspace.getConfiguration('ferro');
    const compilerPath = config.get('compilerPath', 'ferro');
    // Try to find the compiler
    const serverCommand = resolveCompilerPath(compilerPath);
    const serverOptions = {
        run: {
            command: serverCommand,
            args: ['lsp'],
            transport: node_1.TransportKind.stdio,
        },
        debug: {
            command: serverCommand,
            args: ['lsp'],
            transport: node_1.TransportKind.stdio,
        },
    };
    const clientOptions = {
        documentSelector: [{ scheme: 'file', language: 'ferro' }],
        synchronize: {
            fileEvents: vscode.workspace.createFileSystemWatcher('**/*.ferro'),
        },
    };
    client = new node_1.LanguageClient('ferroLanguageServer', 'Ferro Language Server', serverOptions, clientOptions);
    // Start the client (which also starts the server)
    client.start();
    // Register a status bar item
    const statusBar = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
    statusBar.text = '$(flame) Ferro';
    statusBar.tooltip = 'Ferro Language Server is running';
    statusBar.show();
    context.subscriptions.push(statusBar);
    console.log('Ferro extension activated');
}
function deactivate() {
    if (!client) {
        return undefined;
    }
    return client.stop();
}
function resolveCompilerPath(configured) {
    // If it's an absolute path, use it directly
    if (path.isAbsolute(configured)) {
        return configured;
    }
    // If a workspace is open, check for a local build
    const workspaceFolders = vscode.workspace.workspaceFolders;
    if (workspaceFolders) {
        for (const folder of workspaceFolders) {
            // Check for target/debug/ferro.exe (Rust cargo build output)
            const debugPath = path.join(folder.uri.fsPath, 'target', 'debug', 'ferro.exe');
            const releasePath = path.join(folder.uri.fsPath, 'target', 'release', 'ferro.exe');
            try {
                const fs = require('fs');
                if (fs.existsSync(debugPath)) {
                    return debugPath;
                }
                if (fs.existsSync(releasePath)) {
                    return releasePath;
                }
            }
            catch {
                // ignore
            }
        }
    }
    // Fall back to PATH lookup
    return configured;
}
//# sourceMappingURL=extension.js.map