import * as Parser from 'tree-sitter';
import * as LSP from 'vscode-languageserver';
import { TextDocument } from 'vscode-languageserver-textdocument';
export default class TreeSitterAnalyzer {
    static fromBeancountFile(connection: LSP.Connection, rootBeancountFile: string, parser: Parser): TreeSitterAnalyzer;
    private parser;
    private uriToTSTree;
    constructor(parser: Parser);
    getTree(uri: string): Parser.Tree;
    parse(document: TextDocument): void;
}
