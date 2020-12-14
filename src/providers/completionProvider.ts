import { container } from "tsyringe";
import {
    CompletionItem,
    CompletionItemKind,
    CompletionItemTag,
    CompletionList,
    CompletionParams,
    CompletionTriggerKind,
    Connection,
    Position,
    Range,
} from "vscode-languageserver";
import { Tree } from "web-tree-sitter";

import { Forest } from '../forest'
import TreeSitterUtils from '../utils/treesitterUtils'

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

            let contextNode = nodeAtPosition.previousSibling

            // Date is always at the start of a line
            if (isAtStartOfLine
                || (params.position.character == 1 && triggerChar === '2')) {
                return this.getDateCompletions(tree, replaceRange)
            }

            //if (nodeAtPosition.parent && nodeAtPosition.parent.type != "file") this.connection.console.error("parent: " + nodeAtPosition.parent.toString());
            //if (nodeAtLineBefore.type != "file") this.connection.console.error("before:" + nodeAtLineBefore.toString())
            this.connection.console.error(nodeAtPosition.toString())
            //if (nodeAtLineAfter.type != "file") this.connection.console.error("after" + nodeAtLineAfter.toString())
            if (contextNode) this.connection.console.error("context:" + contextNode.toString())
            if (triggerChar) this.connection.console.error("triggerChar: " + triggerChar)

            //this.connection.console.error(nodeAtPosition.toString())

            if (
                ((contextNode && contextNode.type === "txn") ||
                    (nodeAtPosition.type === "string")) &&
                triggerChar === "\""
            ) {
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

            if (
                contextNode && contextNode.type === "string" &&
                triggerChar === "\""
            ) {
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

            if (
                //TreeSitterUtils.findParentOfType("posting_or_kv_list", nodeAtPosition) &&
                nodeAtPosition.type === "identifier" &&
                contextNode && contextNode.type != "date"
            ) {
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

        return [];
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
                insertText: value + "\"",
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
