import * as lsp from 'vscode-languageserver'
import { container } from "tsyringe";
import * as path from 'path'
import { Parser } from 'web-tree-sitter'
import { promisify } from "util";
import * as fs from "fs";
import * as os from 'os'
const readFileAsync = promisify(fs.readFile);

import { Settings } from './utils/settings'
import TreeSitterUtils from './utils/treesitterUtils'
import { Forest } from './forest'
import { ASTProvider } from './providers/astProvider'
import { BeanCheckProvider } from './providers/beanCheckProvider'
import { DocumentFormattingProvider } from './providers/documentFormattingProvider'
import { CompletionProvider } from './providers/completionProvider'

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
        new CompletionProvider();
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
                    resolveProvider: true,
                    triggerCharacters: ['^', ':', '#', '"', '2'],
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
        // TODO add progress bar
        const settings = container.resolve<Settings>("Settings");
        const forest = container.resolve<Forest>("Forest");
        const parser = container.resolve<Parser>("Parser");
        const journalFile = settings.getClientSettings().journalFile;
        const journalUri = `file://${journalFile}`;

        const seenFiles: string[] = []
        seenFiles.push(journalUri)

        const failedFiles: string[] = []

        for (var i = 0; i < seenFiles.length; i++) {
            const fileUri = seenFiles[i]
            const file = fileUri.replace("file://", "").replace("~", os.homedir)

            this.connection.console.info("Parsing ... " + file);
            try {
                const fileContent = await readFileAsync(file, 'utf8');
                const tree = parser.parse(fileContent);
                forest.setTree(fileUri, tree);

                const includeNodes = TreeSitterUtils.findIncludes(tree.rootNode)
                if (includeNodes) {
                    includeNodes.forEach(
                        (includeNode) => {
                            const stringNode = includeNode.children[1]
                            if (stringNode) {
                                const includePath = stringNode.text.replace(/"/g, "")
                                const includeFile = path.join(
                                    path.dirname(file),
                                    includePath
                                )
                                const includeUri = `file://${includeFile}`;
                                
                                if (includeUri.endsWith("*")) {
                                    const filenames = fs.readdirSync(includeUri.replace("file://", "").replace("*", ""));
                                    filenames.forEach(function (fileInFolder) {
                                        const folderFileUir = `file://${fileInFolder}`;
                                        if (!seenFiles.includes(folderFileUir)) {
                                            seenFiles.push(folderFileUir);
                                        } 
                                    });
                                } else if (!seenFiles.includes(includeUri)) {
                                    seenFiles.push(includeUri);
                                }
                            }
                        }
                    );
                }
            } catch (e) {
                console.error(e);
                failedFiles.push(fileUri);
                continue;
            }
        }
        if (failedFiles.length > 0) {
            throw new Error("Not all files are loaded. Error loading files: " + failedFiles);
        }

    }
}

