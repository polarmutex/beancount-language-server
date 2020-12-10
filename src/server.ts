import * as lsp from 'vscode-languageserver'
import { container } from "tsyringe";
import * as path from 'path'
import { Parser } from 'web-tree-sitter'

import { Settings } from './utils/settings'
import TreeSitterUtils from './utils/treesitterUtils'
import { Forest } from './forest'
import { ASTProvider } from './providers/astProvider'
import { BeanCheckProvider } from './providers/beanCheckProvider'
import { DocumentFormattingProvider } from './providers/documentFormattingProvider'

export default class BeancountLspServer {

    private connection: lsp.Connection

    constructor(
        params: lsp.InitializeParams,
        private progress: lsp.WorkDoneProgressReporter
    ) {
        this.connection = container.resolve("Connection");

        const opts = params.initializationOptions;
        const journalFile = opts['journalFile']
        if (journalFile == undefined) {
            this.connection.window.showErrorMessage(
                'Must include journalFile in Initiaize parameters'
            )
        }
    }

    /**
     * Register handlers for the events from the Language Server Protocol that we
     * care about.
     */
    public async register(): Promise<void> {
        container.register("Forest", {
            useValue: new Forest()
        })
        container.register(ASTProvider, {
            useValue: new ASTProvider()
        })
        new BeanCheckProvider();
        new DocumentFormattingProvider();
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
        const settings = container.resolve<Settings>("Settings");
        const forest = container.resolve<Forest>("Forest");
        const parser = container.resolve<Parser>("Parser");
        const journalFile = settings.getClientSettings().journalFile;
        const journalUri = `file://${journalFile}`;

        const seenFiles: string[] = []
        seenFiles.push(journalUri)

        for (var i = 0; i < seenFiles.length; i++) {
            const fileUri = seenFiles[i]
            const file = fileUri.replace("file://", "")
            this.connection.console.info("Parsing ... " + file);
            const tree = parser.parse(fileUri);
            forest.setTree(fileUri, tree);

            const includeNodes = TreeSitterUtils.findIncludeFiles(tree.rootNode)
            if (includeNodes) {
                includeNodes.forEach(
                    (includeNode) => {
                        const includePath = includeNode.text.replace(/"/g, "")
                        const includeFile = path.join(
                            path.dirname(file),
                            includePath
                        )
                        const includeUri = `file://${includeFile}`
                        if (seenFiles.includes(includeUri)) {
                            seenFiles.push(includeUri);
                        }
                    }
                );
            }
        }

    }
}

