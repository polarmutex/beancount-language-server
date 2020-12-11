import { container } from "tsyringe";
import {
    CompletionItem,
    CompletionItemKind,
    CompletionItemTag,
    CompletionList,
    CompletionParams,
    CompletionTriggerKind,
    Connection,
    InsertTextFormat,
    MarkupKind,
    Position,
    Range,
    TextEdit,
} from "vscode-languageserver";
import { URI } from "vscode-uri";
import { SyntaxNode, Tree } from "web-tree-sitter";

import { Forest } from '../forest'
import TreeSitterUtils from '../utils/treesitterUtils'
import { comparePosition, PositionUtil } from '../utils/positionUtils'

export class CompletionProvider {

    private connection: Connection;
    constructor() {
        this.connection = container.resolve<Connection>("Connection");
        this.connection.onCompletion(
            this.handleCompletionRequest.bind(this)
        );
    }

    protected handleCompletionRequest = (
        params: CompletionParams,
    ): CompletionItem[] | CompletionList => {
        const completions: CompletionItem[] = [];
        const forest = container.resolve<Forest>("Forest")
        const treeContainer = forest.getByUri(params.textDocument.uri)

        if (treeContainer) {
            const tree = treeContainer?.tree;
            //console.log(tree.rootNode.toString())

            const nodeAtPosition = TreeSitterUtils.getNamedDescendantForPosition(
                tree.rootNode,
                params.position
            );

            const currentCharacter = params.position.character;

            const targetLine = tree.rootNode.text.split("\n")[params.position.line];
            const containsTrigger = params.context && params.context.triggerKind == CompletionTriggerKind.TriggerCharacter;
            const triggerChar = containsTrigger ?
                params.context?.triggerCharacter :
                targetLine[currentCharacter - 1]

            let replaceRange = Range.create(
                Position.create(params.position.line, currentCharacter),
                params.position,
            );

            const isAtStartOfLine = replaceRange.start.character === 0;

            let targetWord = targetLine.substring(
                replaceRange.start.character,
                replaceRange.end.character,
            );

            let contextNode = TreeSitterUtils.findPreviousNode(
                tree.rootNode,
                params.position,
            );

            if (isAtStartOfLine) {
                // Date is always at the start of a line
                return this.getDateCompletions(tree, replaceRange)
            }
            else {
                if (contextNode?.parent?.hasError()) {
                    // We are completing a type
                    let prevSibling = contextNode.previousSibling?.type;

                    if (prevSibling === "date") {
                        console.log("Next is flag | directive")
                    }
                    else if (prevSibling === "flag") {
                        const payeeOnes: string[] = [];
                        forest.treeMap.forEach((container) => {
                            container.payeeStr1.forEach((value) => {
                                if (!payeeOnes.includes(value)) {
                                    payeeOnes.push(value)
                                }
                            })
                        })
                        return this.getPayeeCompletions(payeeOnes);
                    }
                    else if (contextNode.type === "ERROR" && prevSibling === "string") {
                        const payeeTwos: string[] = [];
                        forest.treeMap.forEach((container) => {
                            container.payeeStr2.forEach((value) => {
                                if (!payeeTwos.includes(value)) {
                                    payeeTwos.push(value)
                                }
                            })
                        })
                        return this.getPayeeCompletions(payeeTwos);
                    }
                    else {
                        console.log("conextNode: " + contextNode.type)
                    }
                }
                else {
                    if (nodeAtPosition.type === "postings" &&
                        nodeAtPosition.parent?.type === "transaction") {
                        const accounts: string[] = [];
                        forest.treeMap.forEach((container) => {
                            container.accountDefinitions.forEach((value) => {
                                if (!accounts.includes(value)) {
                                    accounts.push(value)
                                }
                            })
                        })
                        return this.getAccountCompletions(accounts)
                    }
                }
            }
        }

        return completions;
    }

    private getDateCompletions(
        tree: Tree,
        replaceRange: Range
    ): CompletionItem[] {
        const completions: CompletionItem[] = [];

        const d: Date = new Date();

        const currentYear = d.getFullYear()
        const nextMonth = (d.getMonth() + 2).toString().padStart(2, "0")
        const currentMonth = (d.getMonth() + 1).toString().padStart(2, "0")
        const prevMonth = (d.getMonth()).toString().padStart(2, "0")
        const currentDate = (d.getDate()).toString().padStart(2, "0")

        // Add Current Month, No Date
        completions.push({
            label: `${currentYear}-${currentMonth}-`,
            detail: "Previous Month",
            kind: CompletionItemKind.Value,
        });
        // Add Todays Data
        completions.push({
            label: `${currentYear}-${currentMonth}-${currentDate}`,
            detail: "Today's Date",
            kind: CompletionItemKind.Value,
            preselect: true
        });
        // Add Prev Month, No Date
        completions.push({
            label: `${currentYear}-${prevMonth}-`,
            detail: "Current Month",
            kind: CompletionItemKind.Value,
        });
        // Add Next Month, No Date
        completions.push({
            label: `${currentYear}-${nextMonth}-`,
            detail: "Next Month",
            kind: CompletionItemKind.Value,
        });

        return completions
    }

    private getPayeeCompletions(
        values: string[]
    ): CompletionItem[] {
        const completions: CompletionItem[] = [];

        values.forEach((value) => {
            completions.push({
                label: value,
                kind: CompletionItemKind.Text
            });
        })

        return completions;
    }

    private getAccountCompletions(
        values: string[]
    ): CompletionItem[] {
        const completions: CompletionItem[] = [];

        values.forEach((value) => {
            completions.push({
                label: value,
                kind: CompletionItemKind.Class
            });
        })

        return completions;
    }
}
