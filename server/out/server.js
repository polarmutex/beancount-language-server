"use strict";
var __awaiter = (this && this.__awaiter) || function (thisArg, _arguments, P, generator) {
    function adopt(value) { return value instanceof P ? value : new P(function (resolve) { resolve(value); }); }
    return new (P || (P = Promise))(function (resolve, reject) {
        function fulfilled(value) { try { step(generator.next(value)); } catch (e) { reject(e); } }
        function rejected(value) { try { step(generator["throw"](value)); } catch (e) { reject(e); } }
        function step(result) { result.done ? resolve(result.value) : adopt(result.value).then(fulfilled, rejected); }
        step((generator = generator.apply(thisArg, _arguments || [])).next());
    });
};
Object.defineProperty(exports, "__esModule", { value: true });
const os = require("os");
const path = require("path");
const LSP = require("vscode-languageserver");
const vscode_languageserver_textdocument_1 = require("vscode-languageserver-textdocument");
const utils_1 = require("./utils");
const diagnostics_1 = require("./diagnostics");
/**
 * The BashServer glues together the separate components to implement
 * the various parts of the Language Server Protocol.
 */
class BeancountServer {
    constructor(connection, params) {
        this.documents = new LSP.TextDocuments(vscode_languageserver_textdocument_1.TextDocument);
        connection.console.log(`Initialize: ${params.initializationOptions}`);
        const opts = params.initializationOptions;
        this.rootBeancountFile = opts['rootBeancountFile'].replace("~", os.homedir);
        this.connection = connection;
    }
    /**
     * Initialize the server based on a connection to the client and the protocols
     * initialization parameters.
     */
    static initialize(connection, params) {
        return __awaiter(this, void 0, void 0, function* () {
            const opts = params.initializationOptions;
            const rootBeancountFile = opts['rootBeancountFile'];
            if (rootBeancountFile == undefined) {
                throw new Error('Must include rootBeancountFile in Initiaize parameters');
            }
            return new BeancountServer(connection, params);
        });
    }
    /**
     * Register handlers for the events from the Language Server Protocol that we
     * care about.
     */
    register(connection) {
        this.connection.console.log('******************************  registering');
        // The content of a text document has changed. This event is emitted
        // when the text document first opened or when its content has changed.
        this.documents.listen(this.connection);
        // Register all the handlers for the LSP events.
        connection.onDidSaveTextDocument(this.onDidSaveTextDocument.bind(this));
    }
    /**
     * The parts of the Language Server Protocol that we are currently supporting.
     */
    capabilities() {
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
        };
    }
    logRequest({ request, params, word, }) {
        const wordLog = word ? `"${word}"` : 'null';
        this.connection.console.log(`${request} ${params.position.line}:${params.position.character} word=${wordLog}`);
    }
    onDidSaveTextDocument(params) {
        const beanCheckPy = path.join(__dirname, '../python/bean_check.py');
        const pyArgs = [beanCheckPy, this.rootBeancountFile];
        // TODO: Allow option to specify python path
        utils_1.runExternalCommand('python', pyArgs, (text) => {
            if (text) {
                const output = text.split('\n', 3);
                const errors = output[0];
                const flagged = output[1];
                //this.connection.console.log(errors)
                //this.connection.console.log("\n\n")
                //this.connection.console.log(flagged)
                const diagnostics = diagnostics_1.provideDiagnostics(errors, flagged);
                for (const file of Object.keys(diagnostics)) {
                    const relative_folder = path.relative(path.dirname(this.rootBeancountFile), path.dirname(file));
                    this.connection.sendDiagnostics({
                        uri: 'file://' + relative_folder + path.sep + path.basename(file),
                        diagnostics: diagnostics[file]
                    });
                }
            }
        }, undefined, (str) => {
            this.connection.console.error(str);
        });
    }
}
exports.default = BeancountServer;
//# sourceMappingURL=server.js.map