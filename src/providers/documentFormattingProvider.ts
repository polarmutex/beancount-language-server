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
import Parser, { Edit, Language, Point, Query, QueryResult, SyntaxNode, Tree } from 'web-tree-sitter'

import { Forest } from '../forest'
import { TextDocumentEvents } from '../utils/textDocumentEvents'
import { Settings } from '../utils/settings'
import { compareTSPositions } from '../utils/positionUtils'


interface TSRange {
    start: Point
    end: Point
}
interface Match {
    prefix: TSRange | null;
    number: TSRange | null;
}

export class DocumentFormattingProvider {
    private connection: Connection;
    private documentEvents: TextDocumentEvents;
    private settings: Settings;

    private language: Language;
    private query: Query;

    constructor() {
        this.connection = container.resolve("Connection");
        this.documentEvents = container.resolve("TextDocumentEvents");
        this.settings = container.resolve("Settings");

        this.connection.onDocumentFormatting(
            this.handleDocumentFormatting.bind(this)
        );

        this.language = container.resolve<Parser>("Parser").getLanguage();
        this.query = this.language.query(
            `
            ( posting
                (account) @prefix
                amount: (incomplete_amount
                    [
                        (unary_number_expr)
                        (number)
                    ] @number
                )?
            )
            ( balance
                (account) @prefix
                (amount_tolerance
                    ([
                        (unary_number_expr)
                        (number)
                    ] @number)
                )
            )
            `
        );
    }


    protected async handleDocumentFormatting(
        params: DocumentFormattingParams
    ): Promise<TextEdit[]> {
        const textEdits: TextEdit[] = [];

        const forest = container.resolve<Forest>("Forest")
        const treeContainer = forest.getByUri(params.textDocument.uri)
        if (treeContainer) {

            const tree = treeContainer.tree

            let opts = params.options

            // translate
            if (opts.convertTabsToSpaces === undefined) {
                opts.convertTabsToSpaces = params.options.insertSpaces
            }
            if (opts.indentSize === undefined) {
                opts.indentSize = params.options.tabSize
            }

            const match_pairs: Match[] = this.query.matches(tree.rootNode)
                .map((match) => {

                    let prefix: TSRange | null = null;
                    let number: TSRange | null = null;
                    console.log(match)

                    match.captures.forEach((capture) => {
                        if (capture.name === "prefix") {
                            console.log(capture.node.toString())
                            prefix = {
                                start: capture.node.startPosition,
                                end: capture.node.endPosition
                            }
                            prefix.start.column = 0;
                        }
                        else if (capture.name === "number") {
                            number = {
                                start: capture.node.startPosition,
                                end: capture.node.endPosition
                            }
                        }
                    })
                    if (prefix != null) {
                        //console.log(prefix!.start.column.toString())
                        //console.log(prefix!.end.column.toString())
                    }
                    return {
                        prefix: prefix,
                        number: number,
                    }
                })

            // make sure inital white spaec is lined up
            //const norm_match_pairs = this.normalize_indent_whitespace(match_pairs)

            // find the max width of prefix and numbers
            let max_prefix_width: number = 0;
            let max_number_width: number = 0;
            match_pairs.forEach((match) => {

                if (match.prefix != null && match.number != null) {
                    let len = match.prefix.end.column - match.prefix.start.column
                    if (len > max_prefix_width) {
                        max_prefix_width = len;
                        this.connection.console.error("maxRow: " + match.prefix.start.row.toString() + " - " + len)
                    }
                    //console.log(match.prefix.start.column)
                    //console.log(match.prefix.end.column)
                    len = match.number.end.column - match.number.start.column
                    if (len > max_number_width) {
                        max_number_width = len;
                    }
                }
            });

            const prefixNumberBuffer = 2
            const correct_number_placement = max_prefix_width + prefixNumberBuffer
            match_pairs.forEach((match) => {
                if (match.number && match.prefix) {

                    const numLen = match.number.end.column - match.number.start.column
                    const numColPos = match.number.start.column
                    const newNumPos = correct_number_placement + (max_number_width - numLen)

                    const insertPos: Position = {
                        line: match.prefix.end.row,
                        character: match.prefix.end.column
                    }

                    //console.log("correct_number" + correct_number_placement)
                    //console.log("newPos" + newNumPos)
                    //console.log("numPos" + numColPos)
                    //console.log("numStart" + match.number.start.column)
                    //console.log("numEnd" + match.number.end.column)

                    if (newNumPos > numColPos) {
                        // Insert Spaces
                        const edit: TextEdit = {
                            range: {
                                start: insertPos,
                                end: insertPos
                            },
                            newText: ' '.repeat(newNumPos - numColPos)
                        }
                        textEdits.push(edit)
                    }
                    else if (numColPos > newNumPos) {
                        // remove spaces
                        const endPos: Position = {
                            line: insertPos.line,
                            character: insertPos.character + (numColPos - newNumPos)
                        }
                        const edit: TextEdit = {
                            range: {
                                start: insertPos,
                                end: endPos
                            },
                            newText: ''
                        }
                        textEdits.push(edit)
                    }
                }
            })

            this.connection.console.error(max_prefix_width.toString())
            this.connection.console.error(max_number_width.toString())
            console.log(textEdits)

            return textEdits
        }

        return textEdits;
    }

    /*
     * TODO check initial tabs
    private calc_most_frequent(match_pairs: Match[]): number {
        var frequencies = match_pairs.reduce(
            function(acc, curr) {
                const prefixWidth: number = curr.prefix.startPosition.column
                console.log(curr.prefix.startPosition)
                if (typeof acc[prefixWidth] == 'undefined') {
                    acc[prefixWidth] = 1;
                }
                else {
                    acc[prefixWidth] += 1;
                }
                return acc
            }, {} as Record<number, number>
        );

        let width = 0;
        let occurences = 0;

        for (let [k, v] of Object.entries(frequencies)) {
            if (v >= occurences) {
                width = v;
            }
        }
        return width;
    }

    private normalize_indent_whitespace(match_pairs: Match[]): Match[] {
        const width = this.calc_most_frequent(match_pairs)
        const norm_format = ' '.repeat(width) + '{}';
        console.log(width)
        console.log(norm_format)

        const adjusted_pairs: Match[] = [];
        match_pairs.forEach((match) => {
        })
        return adjusted_pairs;
    }*/
}
