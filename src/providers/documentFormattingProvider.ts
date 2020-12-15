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

interface Match {
    prefix: SyntaxNode[];
    number: SyntaxNode[];
    rest: SyntaxNode[];
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
            console.log(tree.rootNode.toString())

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
                    console.log(match)
                    const prefixNodes: SyntaxNode[] = [];
                    const numberNodes: SyntaxNode[] = [];
                    const restNodes: SyntaxNode[] = [];
                    match.captures.forEach((capture) => {
                        if (capture.name === "prefix") {
                            prefixNodes.push(capture.node)
                        }
                        if (capture.name === "number") {
                            numberNodes.push(capture.node)
                        }
                        if (capture.name === "rest") {
                            restNodes.push(capture.node)
                        }
                    })
                    return {
                        prefix: prefixNodes,
                        number: numberNodes,
                        rest: restNodes
                    }
                })

            // make sure inital white spaec is lined up
            //const norm_match_pairs = this.normalize_indent_whitespace(match_pairs)

            // find the max width of prefix and numbers
            let max_prefix_width: number = 0;
            let max_number_width: number = 0;
            match_pairs.forEach((match) => {
                if (match.prefix.length > 0 && match.number.length > 0) {
                    let maxPrefix = 0;
                    match.prefix.forEach((p) => {
                        if (p.endPosition.column > maxPrefix) {
                            maxPrefix = p.endPosition.column
                        }
                    })
                    if (maxPrefix > max_prefix_width) {
                        max_prefix_width = maxPrefix
                    }

                    let maxNumber = 0;
                    match.number.forEach((n) => {
                        if (n.endPosition.column > maxPrefix) {
                            maxNumber = n.endPosition.column
                        }
                    })
                    if (maxNumber > max_number_width) {
                        max_number_width = maxNumber
                    }
                }
            });

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
