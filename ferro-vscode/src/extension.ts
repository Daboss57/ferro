import * as path from 'path';
import * as vscode from 'vscode';
import {
    LanguageClient,
    LanguageClientOptions,
    ServerOptions,
    TransportKind,
} from 'vscode-languageclient/node';

let client: LanguageClient | undefined;

export function activate(context: vscode.ExtensionContext) {
    // Get the compiler path from settings
    const config = vscode.workspace.getConfiguration('ferro');
    const compilerPath = config.get<string>('compilerPath', 'ferro');

    // Try to find the compiler
    const serverCommand = resolveCompilerPath(compilerPath);

    const serverOptions: ServerOptions = {
        run: {
            command: serverCommand,
            args: ['lsp'],
            transport: TransportKind.stdio,
        },
        debug: {
            command: serverCommand,
            args: ['lsp'],
            transport: TransportKind.stdio,
        },
    };

    const clientOptions: LanguageClientOptions = {
        documentSelector: [{ scheme: 'file', language: 'ferro' }],
        synchronize: {
            fileEvents: vscode.workspace.createFileSystemWatcher('**/*.ferro'),
        },
    };

    client = new LanguageClient(
        'ferroLanguageServer',
        'Ferro Language Server',
        serverOptions,
        clientOptions
    );

    // Start the client (which also starts the server)
    client.start();

    // Register a status bar item
    const statusBar = vscode.window.createStatusBarItem(
        vscode.StatusBarAlignment.Left,
        100
    );
    statusBar.text = '$(flame) Ferro';
    statusBar.tooltip = 'Ferro Language Server is running';
    statusBar.show();
    context.subscriptions.push(statusBar);

    console.log('Ferro extension activated');
}

export function deactivate(): Thenable<void> | undefined {
    if (!client) {
        return undefined;
    }
    return client.stop();
}

function resolveCompilerPath(configured: string): string {
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
            } catch {
                // ignore
            }
        }
    }

    // Fall back to PATH lookup
    return configured;
}
