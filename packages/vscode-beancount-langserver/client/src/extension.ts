/* --------------------------------------------------------------------------------------------
 * Copyright (c) Microsoft Corporation. All rights reserved.
 * Licensed under the MIT License. See License.txt in the project root for license information.
 * ------------------------------------------------------------------------------------------ */

import * as path from 'path';
import * as vscode from 'vscode';

import {
	LanguageClient,
	LanguageClientOptions,
	ServerOptions,
	TransportKind
} from 'vscode-languageclient/node';

import {
    SemanticTokensProvider,
    buildLegend
} from './semanticToken'

let client: LanguageClient;
let logger = vscode.window.createOutputChannel('Beancount-LangServer')

export interface IClientSettings {
    journalFile:string
    pythonPath:string
}

const config = vscode.workspace.getConfiguration().get<IClientSettings>("beancountLangServer")

function getSettings(config: IClientSettings) {
    return config ?
        {
            journalFile: config.journalFile,
            pythonPath: config.pythonPath
        }
        : {};
}

export async function activate(context: vscode.ExtensionContext) {
	// The server is implemented in node
	let serverModule = context.asAbsolutePath(
		path.join('server', 'out', 'cli.js')
	);
	// The debug options for the server
	// --inspect=6009: runs the server in Node's Inspector mode so VS Code can attach to the server for debugging
	let debugOptions = { execArgv: ['--nolazy', `--inspect=6009`] };

	// If the extension is launched in debug mode then the debug server options are used
	// Otherwise the run options are used
	let serverOptions: ServerOptions = {
		run: { module: serverModule, transport: TransportKind.ipc },
		debug: {
			module: serverModule,
			transport: TransportKind.stdio,
			options: debugOptions
		}
	};

	// Options to control the language client
	let clientOptions: LanguageClientOptions = {
		// Register the server for plain text documents
		documentSelector: [{language: 'beancount' }],
		synchronize: {
			// Notify the server about file changes to '.clientrc files contained in the workspace
			fileEvents: vscode.workspace.createFileSystemWatcher('**/.beancount')
		},
        initializationOptions: getSettings(config)
	};

	// Create the language client and start the client.
	client = new LanguageClient(
		'beancountLangServer',
		'Beancount Language Server',
		serverOptions,
		clientOptions
	);

	// Start the client. This will also launch the server
	client.start();

	const legend = buildLegend();
	const tokenProvider = new SemanticTokensProvider(legend);
	await tokenProvider.ast.init();

	const enabledLangs: string[] =
		vscode.workspace.getConfiguration("syntax").get("highlightLanguages");

    context.subscriptions.push(
		vscode.languages.registerDocumentSemanticTokensProvider(
        {language: 'beancount'},
        tokenProvider,
        legend
	));
}

export function deactivate(): Thenable<void> | undefined {
	if (!client) {
		return undefined;
	}
	return client.stop();
}
