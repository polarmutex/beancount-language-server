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
import Parser, { Edit, Point, Tree } from 'web-tree-sitter'

import { Forest } from '../forest'
import { TextDocumentEvents } from '../utils/textDocumentEvents'

export class ASTProvider {
    private connection: Connection;
    private parser: Parser;
    private documentEvents: TextDocumentEvents;
    private forest: Forest;

    constructor() {
        this.connection = container.resolve("Connection");
        this.parser = container.resolve("Parser");
        this.documentEvents = container.resolve("TextDocumentEvents");
        this.forest = container.resolve("Forest");

        this.documentEvents.on(
            "open",
            this.handleChangeTextDocument.bind(this)
        )

        this.documentEvents.on(
            "change",
            this.handleChangeTextDocument.bind(this)
        )
    }

    protected handleChangeTextDocument = (
        params: DidOpenTextDocumentParams | DidChangeTextDocumentParams
    ): void => {
        const document = params.textDocument;

        let tree: Tree | undefined = this.forest.getTree(document.uri)


        // TODO incremental syncing not working
        // tree seems to get corrupted

        let hasContentChanges = false
        if ("contentChanges" in params) {
            hasContentChanges = true
            for (const change of params.contentChanges) {
                if ("range" in change) {
                    console.info(change)
                    tree?.edit(this.getEditFromChange(change, tree.rootNode.text));
                }
            }
        }

        const newText =
            this.documentEvents.get(document.uri)?.getText() ??
            readFileSync(URI.parse(document.uri).fsPath, "utf8");

        const newTree = this.parser.parse(
            newText,
            hasContentChanges ? tree : undefined
        );

        tree = newTree

        if (tree) {
            this.forest.setTree(
                document.uri,
                tree
            );
        }
    }

    private getEditFromChange(
        change: { text: string, range: Range },
        text: string
    ): Edit {
        const [startIndex, endIndex] = this.getIndexesFromRange(change.range, text);

        return {
            startIndex,
            oldEndIndex: endIndex,
            newEndIndex: startIndex + change.text.length,
            startPosition: this.toTSPoint(change.range.start),
            oldEndPosition: this.toTSPoint(change.range.end),
            newEndPosition: this.toTSPoint(
                this.addPositions(
                    change.range.start,
                    this.textToPosition(change.text)
                )
            )
        }
    }

    private textToPosition(text: string): Position {
        const lines = text.split(/\r\n|\r|\n/);

        return {
            line: lines.length - 1,
            character: lines[lines.length - 1].length,
        };
    }

    private getIndexesFromRange(range: Range, text: string): [number, number] {
        let startIndex = range.start.character;
        let endIndex = range.end.character;

        const regex = new RegExp(/\r\n|\r|\n/);
        const eolResult = regex.exec(text);

        const lines = text.split(regex);
        const eol = eolResult && eolResult.length > 0 ? eolResult[0] : "";

        for (let i = 0; i < range.end.line; i++) {
            if (i < range.start.line) {
                startIndex += lines[i].length + eol.length;
            }
            endIndex += lines[i].length + eol.length;
        }

        return [startIndex, endIndex];
    }

    private addPositions(pos1: Position, pos2: Position): Position {
        return {
            line: pos1.line + pos2.line,
            character: pos1.character + pos2.character,
        };
    }

    private toTSPoint(position: Position): Point {
        return { row: position.line, column: position.character };
    }
}
