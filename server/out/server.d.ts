import * as LSP from 'vscode-languageserver';
/**
 * The BashServer glues together the separate components to implement
 * the various parts of the Language Server Protocol.
 */
export default class BeancountServer {
    /**
     * Initialize the server based on a connection to the client and the protocols
     * initialization parameters.
     */
    static initialize(connection: LSP.Connection, params: LSP.InitializeParams): Promise<BeancountServer>;
    private documents;
    private connection;
    private rootBeancountFile;
    private constructor();
    /**
     * Register handlers for the events from the Language Server Protocol that we
     * care about.
     */
    register(connection: LSP.Connection): void;
    /**
     * The parts of the Language Server Protocol that we are currently supporting.
     */
    capabilities(): LSP.ServerCapabilities;
    private logRequest;
    private onDidSaveTextDocument;
}
