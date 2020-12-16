import {
    Connection,
    DidOpenTextDocumentParams,
    DidChangeTextDocumentParams
} from 'vscode-languageserver'
import {
    Position,
    Range
} from 'vscode-languageserver-textdocument'
import { URI } from 'vscode-uri'
import { readFileSync } from 'fs';
import { container } from 'tsyringe';
import * as path from 'path'
import Parser, { Edit, Point, Tree } from 'web-tree-sitter'

import { Forest } from '../forest'
import { TextDocumentEvents } from '../utils/textDocumentEvents'
import { Settings } from '../utils/settings'
import { runExternalCommand } from '../utils/runExternalCmd'
import { provideDiagnostics } from '../utils/diagnostics'

export class BeanCheckProvider {
    private connection: Connection;
    private documentEvents: TextDocumentEvents;
    private settings: Settings;

    constructor() {
        this.connection = container.resolve("Connection");
        this.documentEvents = container.resolve("TextDocumentEvents");
        this.settings = container.resolve("Settings");

        this.documentEvents.on(
            "open",
            this.runBeanCheck.bind(this)
        )

        this.documentEvents.on(
            "save",
            this.runBeanCheck.bind(this)
        )
    }

    protected async runBeanCheck(): Promise<void> {
        const journalFile = this.settings.getClientSettings().journalFile
        const beanCheckPy = path.join(__dirname, '../../python/bean_check.py');
        const pyArgs = [beanCheckPy, journalFile]
        // TODO: Allow option to specify python path
        this.connection.console.error("journalFile: " + journalFile)
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
                path.dirname(journalFile),
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
}
