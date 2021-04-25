import { SyntaxNode, Tree } from 'web-tree-sitter'
import TreeSitterUtils from './utils/treesitterUtils'

export interface ITreeContainer {
    uri: string;
    tree: Tree;
    payeeStr1: string[];
    payeeStr2: string[];
    accountDefinitions: string[];
}

export interface IForest {
    treeMap: Map<string, ITreeContainer>;
    getTree(uri: string): Tree | undefined;
    getByUri(uri: string): ITreeContainer | undefined;
    setTree(
        uri: string,
        tree: Tree
    ): ITreeContainer;
    removeTree(uri: string): void;
}

export class Forest implements IForest {
    public treeMap: Map<string, ITreeContainer> = new Map<string, ITreeContainer>();

    constructor() {
    }

    public getTree(uri: string): Tree | undefined {
        return this.getByUri(uri)?.tree;
    }

    public getByUri(uri: string): ITreeContainer | undefined {
        return this.treeMap.get(uri);
    }

    public setTree(
        uri: string,
        tree: Tree
    ): ITreeContainer {
        // Make sure uri is populated
        tree.uri = uri

        const transactions = TreeSitterUtils.findTransactions(tree.rootNode);
        const opens = TreeSitterUtils.findOpens(tree.rootNode);

        const treeContainer: ITreeContainer = {
            uri: uri,
            tree: tree,
            payeeStr1: this.getPayeeOneStrings(transactions),
            payeeStr2: this.getPayeeTwoStrings(transactions),
            accountDefinitions: this.getAccountDefinitions(opens)
        }
        this.treeMap.set(uri, treeContainer);
        return treeContainer
    }

    public removeTree(uri: string): void {
        const exists = this.getByUri(uri);
        if (exists) {
            this.treeMap.delete(uri);
        }
    }

    private getPayeeOneStrings(txns: SyntaxNode[] | undefined): string[] {
        if (txns) {
            const values: string[] = [];
            txns.forEach((txn) => {
                const txnStrings = txn.childForFieldName('txn_strings')
                const payee1 = txnStrings?.children[0].text.replace(/"/g, "")
                if (payee1) {
                    if (!values.includes(payee1)) {
                        values.push(payee1)
                    }
                }
            });
            return values;
        }
        return [];
    }

    private getPayeeTwoStrings(txns: SyntaxNode[] | undefined): string[] {
        if (txns) {
            const values: string[] = [];
            txns.forEach((txn) => {
                const txnStrings = txn.childForFieldName('txn_strings')
                let payee2 = undefined;
                if (txnStrings && txnStrings.children.length > 1) {
                    payee2 = txnStrings?.children[1].text.replace(/"/g, "")
                }
                if (payee2) {
                    if (!values.includes(payee2)) {
                        values.push(payee2)
                    }
                }
            });
            return values;
        }
        return [];
    }

    private getAccountDefinitions(opens: SyntaxNode[] | undefined): string[] {
        if (opens) {
            const values: string[] = [];
            opens.forEach((open) => {
                const account = open.childForFieldName('account')?.text
                if (account) {
                    if (!values.includes(account)) {
                        values.push(account)
                    }
                }
            });
            return values;
        }
        return [];
    }
}
