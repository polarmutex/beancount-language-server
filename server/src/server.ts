import * as path from 'path'
import * as LSP from 'vscode-languageserver'
import { TextDocument } from 'vscode-languageserver-textdocument'

import { runExternalCommand } from './utils'
/**
 * The BashServer glues together the separate components to implement
 * the various parts of the Language Server Protocol.
 */
export default class BeancountServer {
    /**
     * Initialize the server based on a connection to the client and the protocols
     * initialization parameters.
     */
    public static async initialize(
        connection: LSP.Connection,
        params: LSP.InitializeParams,
    ): Promise<BeancountServer> {
        const opts = params.initializationOptions;
        const rootBeancountFile = opts['rootBeancountFile']
        if (rootBeancountFile == undefined) {
            throw new Error('Must include rootBeancountFile in Initiaize parameters')
        }
        return new BeancountServer(connection, params);
    }


    private documents: LSP.TextDocuments<TextDocument> = new LSP.TextDocuments(TextDocument)
    private connection: LSP.Connection

    private rootBeancountFile: string;

    private constructor(
        connection: LSP.Connection,
        params: LSP.InitializeParams
    ) {
        connection.console.log(`Initialize: ${params.initializationOptions}`)
        const opts = params.initializationOptions;

        this.rootBeancountFile = opts['rootBeancountFile']
        this.connection = connection
    }

    /**
     * Register handlers for the events from the Language Server Protocol that we
     * care about.
     */
    public register(connection: LSP.Connection): void {
        // The content of a text document has changed. This event is emitted
        // when the text document first opened or when its content has changed.
        this.documents.listen(this.connection)

        // Register all the handlers for the LSP events.
        this.connection.onDidSaveTextDocument(this.onDidSaveTextDocument.bind(this));
    }

    /**
     * The parts of the Language Server Protocol that we are currently supporting.
     */
    public capabilities(): LSP.ServerCapabilities {
        return {
            textDocumentSync: LSP.TextDocumentSyncKind.Full,
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

    private onDidSaveTextDocument(
        params: LSP.DidSaveTextDocumentParams
    ) {
        const beanCheckPy = path.resolve('python/bean_check.py')
        const pyArgs = [beanCheckPy, this.rootBeancountFile]
        runExternalCommand(
            'python',
            pyArgs,
            (text: string) => {
            },
            undefined,
            (str: string) => {
                console.log(str)
            }
        );
    }

}

