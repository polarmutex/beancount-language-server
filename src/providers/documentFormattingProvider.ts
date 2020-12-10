import {
    Connection,
    DocumentFormattingParams,
    TextEdit
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

export class DocumentFormattingProvider {
    private connection: Connection;
    private documentEvents: TextDocumentEvents;
    private settings: Settings;

    constructor() {
        this.connection = container.resolve("Connection");
        this.documentEvents = container.resolve("TextDocumentEvents");
        this.settings = container.resolve("Settings");

        this.connection.onDocumentFormatting(
            this.handleDocumentFormatting.bind(this)
        );
    }

    protected async handleDocumentFormatting(
        params: DocumentFormattingParams
    ): Promise<TextEdit[]> {
        const file = this.documentEvents.get(params.textDocument.uri)

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
