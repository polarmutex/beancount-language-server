import { Tree } from 'web-tree-sitter'

export interface ITreeContainer {
    uri: string;
    tree: Tree;
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
        tree.uri = uri
        const treeContainer: ITreeContainer = {
            uri: uri,
            tree: tree
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
}
