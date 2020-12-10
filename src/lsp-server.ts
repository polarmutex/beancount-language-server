import * as os from 'os'
import * as path from 'path'
import * as util from 'util'
import * as LSP from 'vscode-languageserver'
import { TextDocument } from 'vscode-languageserver-textdocument'
import * as Parser from 'web-tree-sitter'
import TreeSitterAnalyzer from './tree-sitter'

import { initializeParser } from './parser'
import { runExternalCommand } from './utils'
import { provideDiagnostics } from './diagnostics'
/**
 * The BashServer glues together the separate components to implement
 * the various parts of the Language Server Protocol.
 */
export default class BeancountLspServer {
    /**
     * Initialize the server based on a connection to the client and the protocols
     * initialization parameters.
     */
    public static async initialize(
        connection: LSP.Connection,
        params: LSP.InitializeParams,
    ): Promise<BeancountLspServer> {

        const parser = await initializeParser();

        const opts = params.initializationOptions;
        const rootBeancountFile = opts['rootBeancountFile'].replace("~", os.homedir)
        if (rootBeancountFile == undefined) {
            throw new Error('Must include rootBeancountFile in Initiaize parameters')
        }

        return Promise.all([
            TreeSitterAnalyzer.fromBeancountFile(connection, rootBeancountFile, parser)
        ]).then(xs => {
            const analyzer = xs[0];
            return new BeancountLspServer(connection, params, analyzer);
        })
    }


    private documents: LSP.TextDocuments<TextDocument> = new LSP.TextDocuments(TextDocument)
    private connection: LSP.Connection
    private rootBeancountFile: string;
    private analyzer: TreeSitterAnalyzer;

    private constructor(
        connection: LSP.Connection,
        params: LSP.InitializeParams,
        analyzer: TreeSitterAnalyzer,
    ) {
        const opts = params.initializationOptions;

        this.rootBeancountFile = opts['rootBeancountFile'].replace("~", os.homedir)
        this.connection = connection
        this.analyzer = analyzer;
    }

    async onInitialize(params: LSP.InitializeParams): Promise<LSP.InitializeResult> {
        //this.connection.console.log(`initialized server v. ${pkg.version}`);
        return {
            capabilities: this.capabilities()
        }
    }

    /**
     * Register handlers for the events from the Language Server Protocol that we
     * care about.
     */
    public register(connection: LSP.Connection): void {
        // The content of a text document has changed. This event is emitted
        // when the text document first opened or when its content has changed.
        this.documents.listen(this.connection)
        this.documents.onDidChangeContent(change => {
            this.analyzer.analyze(change.document.uri, change.document)
        });

        // Register all the handlers for the LSP events.
        connection.onDidOpenTextDocument(this.onDidOpenTextDocument.bind(this));
        connection.onDidSaveTextDocument(this.onDidSaveTextDocument.bind(this));
        connection.onDocumentFormatting(this.onDocumentFormatting.bind(this));
    }

    /**
     * The parts of the Language Server Protocol that we are currently supporting.
     */
    public capabilities(): LSP.ServerCapabilities {
        return {
            textDocumentSync: {
                openClose: true,
                change: LSP.TextDocumentSyncKind.Incremental,
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

    private logRequest({
        request,
        params,
        word,
    }: {
        request: string
        params: LSP.ReferenceParams | LSP.TextDocumentPositionParams
        word?: string | null
    }) {
        const wordLog = word ? `"${word}"` : 'null'
        this.connection.console.log(
            `${request} ${params.position.line}:${params.position.character} word=${wordLog}`,
        )
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
                uri: 'file://' + relative_folder + path.sep + path.basename(file),
                diagnostics: diagnostics[file]
            });
        }
        return
    }

    async onDidOpenTextDocument(
        params: LSP.DidOpenTextDocumentParams
    ): Promise<void> {
        return this.requestDiagnostics()
    }

    async onDidSaveTextDocument(
        params: LSP.DidSaveTextDocumentParams
    ): Promise<void> {
        return this.requestDiagnostics()
    }

    private async onDocumentFormatting(
        params: LSP.DocumentFormattingParams
    ): Promise<LSP.TextEdit[]> {

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

