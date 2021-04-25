import { SyntaxNode, Tree } from 'web-tree-sitter';
import { Position, Range } from 'vscode-languageserver'
import { ITreeContainer } from '../forest'
import { comparePosition } from './positionUtils'

export default class TreeSitterUtils {

    public static isIdentifier(node: SyntaxNode): boolean {
        return (
            node.type === "lower_case_identifier" ||
            node.type === "upper_case_identifier"
        );
    }

    public static findIncludes(node: SyntaxNode): SyntaxNode[] | undefined {
        const result = node.children.filter(
            (child) => child.type === 'include'
        );
        return result.length === 0 ? undefined : result;
    }

    public static findTransactions(node: SyntaxNode): SyntaxNode[] | undefined {
        const result = node.children.filter(
            (child) => child.type === 'transaction'
        );
        return result.length === 0 ? undefined : result;
    }

    public static findOpens(node: SyntaxNode): SyntaxNode[] | undefined {
        const result = node.children.filter(
            (child) => child.type === 'open'
        );
        return result.length === 0 ? undefined : result;
    }

    public static getNamedDescendantForPosition(
        node: SyntaxNode,
        position: Position,
    ): SyntaxNode {
        const previousCharColumn =
            position.character === 0 ? 0 : position.character - 1;

        const charBeforeCursor = node.text
            .split("\n")
        [position.line].substring(previousCharColumn, position.character);

        return node.namedDescendantForPosition(
            {
                column: previousCharColumn,
                row: position.line,
            },
            {
                column: position.character,
                row: position.line,
            },
        );
    }

    public static getNamedDescendantForRange(
        sourceFile: ITreeContainer,
        range: Range,
    ): SyntaxNode {
        return sourceFile.tree.rootNode.namedDescendantForPosition(
            {
                column: range.start.character,
                row: range.start.line,
            },
            {
                column: range.end.character,
                row: range.end.line,
            },
        );
    }

    public static findPreviousNode(
        node: SyntaxNode,
        position: Position,
    ): SyntaxNode | undefined {
        function nodeHasTokens(n: SyntaxNode): boolean {
            return n.endIndex - n.startIndex !== 0;
        }

        function findRightmostChildWithTokens(
            childrenList: SyntaxNode[],
            startIndex: number,
        ): SyntaxNode | undefined {
            for (let i = startIndex - 1; i >= 0; i--) {
                if (nodeHasTokens(childrenList[i])) {
                    return childrenList[i];
                }
            }
        }

        function findRightmostNode(n: SyntaxNode): SyntaxNode | undefined {
            if (n.children.length === 0) {
                return n;
            }

            const candidate = findRightmostChildWithTokens(
                n.children,
                n.children.length,
            );

            if (candidate) {
                return findRightmostNode(candidate);
            }
        }

        const children = node.children;

        if (children.length === 0) {
            return node;
        }

        for (let i = 0; i < children.length; i++) {
            const child = children[i];
            if (comparePosition(position, child.endPosition) < 0) {
                const lookInPreviousChild =
                    comparePosition(position, child.startPosition) <= 0 ||
                    !nodeHasTokens(child);

                if (lookInPreviousChild) {
                    const candidate = findRightmostChildWithTokens(children, i);
                    if (candidate) {
                        return findRightmostNode(candidate);
                    }
                } else {
                    return this.findPreviousNode(child, position);
                }
            }
        }

        const candidate = findRightmostChildWithTokens(children, children.length);
        if (candidate) {
            return findRightmostNode(candidate);
        }
    }

    public static getNamedDescendantForLineBeforePosition(
        node: SyntaxNode,
        position: Position,
    ): SyntaxNode {
        const previousLine = position.line === 0 ? 0 : position.line - 1;

        return node.namedDescendantForPosition({
            column: 0,
            row: previousLine,
        });
    }

    public static getNamedDescendantForLineAfterPosition(
        node: SyntaxNode,
        position: Position,
    ): SyntaxNode {
        const followingLine = position.line + 1;

        return node.namedDescendantForPosition({
            column: 0,
            row: followingLine,
        });
    }

    public static findParentOfType(
        typeToLookFor: string,
        node: SyntaxNode,
        topLevel = false,
    ): SyntaxNode | undefined {
        if (
            node.type === typeToLookFor &&
            (!topLevel || node.parent?.type === "file")
        ) {
            return node;
        }
        if (node.parent) {
            return this.findParentOfType(typeToLookFor, node.parent, topLevel);
        }
    }
}
