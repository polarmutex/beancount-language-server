import * as os from 'os'
import * as path from 'path'
import * as util from 'util'
import * as lsp from 'vscode-languageserver'
import { TextDocument } from 'vscode-languageserver-textdocument'
import * as Parser from 'web-tree-sitter'
import TreeSitterAnalyzer from './tree-sitter'
import { container } from "tsyringe";

import { initializeParser } from './parser'
import { runExternalCommand } from './utils'
import { provideDiagnostics } from './diagnostics'
/**
 * The BashServer glues together the separate components to implement
 * the various parts of the Language Server Protocol.
 */
export default class BeancountLspServer {

    private documents: lsp.TextDocuments<TextDocument> = new lsp.TextDocuments(TextDocument)
    private connection: lsp.Connection
    private rootBeancountFile: string;

    constructor(
        params: lsp.InitializeParams,
        private progress: lsp.WorkDoneProgressReporter
    ) {
        this.connection = container.resolve("Connection");

        const opts = params.initializationOptions;
        this.rootBeancountFile = opts['rootBeancountFile'].replace("~", os.homedir)
        if (this.rootBeancountFile == undefined) {
            this.connection.window.showErrorMessage(
                'Must include rootBeancountFile in Initiaize parameters'
            )
        }
    }

    /**
     * Register handlers for the events from the Language Server Protocol that we
     * care about.
     */
    public async register(): Promise<void> {
    }

    /**
     * The parts of the Language Server Protocol that we are currently supporting.
     */
    get capabilities(): lsp.InitializeResult {
        return {
            capabilities: {
                textDocumentSync: {
                    openClose: true,
                    change: lsp.TextDocumentSyncKind.Incremental,
                    willSave: false,
                    willSaveWaitUntil: false,
                    save: {
                        includeText: false
                    }
                },
                documentFormattingProvider: true,
                completionProvider: {
                    resolveProvider: false,
                    triggerCharacters: [':'],
                },
                hoverProvider: false,
                documentHighlightProvider: false,
                definitionProvider: false,
                documentSymbolProvider: false,
                workspaceSymbolProvider: false,
                referencesProvider: false,
            }
        }
    }

    async init(): Promise<void> {
    }

    async requestDiagnostics(): Promise<void> {
        const beanCheckPy = path.join(__dirname, '../python/bean_check.py');
        const pyArgs = [beanCheckPy, this.rootBeancountFile]
        // TODO: Allow option to specify python path
        const text = await runExternalCommand(
            'python',
            pyArgs,
            undefined,
            (str: string) => {
                this.connection.console.error(str)
                console.log(str)
            }
        );
        const output = text.split('\n', 3);
        const errors = output[0]
        const flagged = output[1]
        const diagnostics = provideDiagnostics(errors, flagged);

        for (const file of Object.keys(diagnostics)) {
            const relative_folder = path.relative(
                path.dirname(this.rootBeancountFile),
                path.dirname(file)
            );
            this.connection.sendDiagnostics({
                //uri: 'file://' + relative_folder + path.sep + path.basename(file),
                uri: `file://${file}`,
                diagnostics: diagnostics[file]
            });
        }
        return
    }

    async onDidOpenTextDocument(
        params: lsp.DidOpenTextDocumentParams
    ): Promise<void> {
        return this.requestDiagnostics()
    }

    async onDidSaveTextDocument(
        params: lsp.DidSaveTextDocumentParams
    ): Promise<void> {
        return this.requestDiagnostics()
    }

    private async onDocumentFormatting(
        params: lsp.DocumentFormattingParams
    ): Promise<lsp.TextEdit[]> {

        const file = this.documents.get(params.textDocument.uri)

        if (!file) {
            return [];
        }

        let opts = params.options

        // translate
        if (opts.convertTabsToSpaces === undefined) {
            opts.convertTabsToSpaces = params.options.insertSpaces
        }
        if (opts.indentSize === undefined) {
            opts.indentSize = params.options.tabSize
        }

        return [];
    }

}

