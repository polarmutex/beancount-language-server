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
    rest: TSRange | null;
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
                        (
                            (optflag)?
                            (account)
                        ) @prefix
                        (
                            (incomplete_amount
                                    [
                                        (unary_number_expr)
                                        (number)
                                    ] @number
                                    (currency)? @rest
                            )?
                        )
                        (
                            (cost_spec)?
                            (at)?
                            (atat)?
                            (price_annotation)?
                        ) @rest
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
                    let rest: TSRange | null = null;

                    match.captures.forEach((capture) => {
                        let temp: TSRange | null = null;
                        if (
                            capture.name === "prefix" ||
                            capture.name === "number" ||
                            capture.name === "rest"
                        ) {
                            if (capture.name === "prefix") {
                                temp = prefix
                            }
                            else if (capture.name === "number") {
                                temp = number
                            }
                            else if (capture.name === "rest") {
                                temp = rest
                            }

                            if (temp == null) {
                                temp = {
                                    start: capture.node.startPosition,
                                    end: capture.node.endPosition
                                }
                            }
                            else {
                                if (compareTSPositions(
                                    capture.node.startPosition,
                                    temp.start
                                ) < 0) {
                                    temp.start = capture.node.startPosition
                                }
                                if (compareTSPositions(
                                    capture.node.endPosition,
                                    temp.end
                                ) > 0) {
                                    temp.end = capture.node.endPosition
                                }
                            }

                            if (capture.name === "prefix") {
                                prefix = temp
                            }
                            else if (capture.name === "number") {
                                number = temp
                            }
                            else if (capture.name === "rest") {
                                rest = temp
                            }
                        }
                    })
                    return {
                        prefix: prefix,
                        number: number,
                        rest: rest
                    }
                })

            // make sure inital white spaec is lined up
            //const norm_match_pairs = this.normalize_indent_whitespace(match_pairs)

            // find the max width of prefix and numbers
            let max_prefix_width: number = 0;
            let max_number_width: number = 0;
            match_pairs.forEach((match) => {
                if (match.prefix != null) {
                    const len = match.prefix.end.column - match.prefix.start.column
                    if (len > max_prefix_width) {
                        max_prefix_width = len;
                    }
                }

                if (match.number != null) {
                    const len = match.number.end.column - match.number.start.column
                    if (len > max_number_width) {
                        max_number_width = len;
                    }
                }
            });

            const tabLen = 4;
            const prefixNumberBuffer = 2
            const correct_number_placement = tabLen + max_prefix_width + prefixNumberBuffer
            match_pairs.forEach((match) => {
                if (match.number && match.prefix) {
                    const numColPos = match.number.start.column
                    const insertPos: Position = {
                        line: match.prefix.end.row,
                        character: match.prefix.end.column + 1
                    }
                    console.log(correct_number_placement)
                    console.log(numColPos)
                    if (numColPos < correct_number_placement) {
                        // Insert Spaces
                        const edit: TextEdit = {
                            range: {
                                start: insertPos,
                                end: insertPos
                            },
                            newText: ' '.repeat(correct_number_placement - numColPos)
                        }
                        textEdits.push(edit)
                    }
                    else if (numColPos > correct_number_placement) {
                        // remove spaces
                        const endPos: Position = {
                            line: insertPos.line,
                            character: insertPos.character + (numColPos - correct_number_placement)
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

            console.log(max_prefix_width)
            console.log(max_number_width)
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
