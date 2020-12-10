import { SyntaxNode, Tree } from 'web-tree-sitter';

export default class TreeSitterUtils {

    public static findIncludeFiles(node: SyntaxNode): SyntaxNode[] | undefined {
        const result = node.children.filter(
            (child) => child.type === 'include'
        );
        return result.length === 0 ? undefined : result;
    }
}
