import * as Parser from 'web-tree-sitter';
import * as LSP from 'vscode-languageserver'
import { TextDocument } from 'vscode-languageserver-textdocument'

export default class TreeSitterAnalyzer {

    public static fromBeancountFile(
        connection: LSP.Connection,
        rootBeancountFile: string,
        parser: Parser
    ): TreeSitterAnalyzer {
        const analyzer = new TreeSitterAnalyzer(parser);
        // TODO parse from root file
        return analyzer;
    }

    private parser: Parser;
    private uriToTSTree: { [uri: string]: Parser.Tree } = {}

    public constructor(parser: Parser) {
        this.parser = parser
    }

    public getTree(uri: string): Parser.Tree {
        return this.uriToTSTree[uri];
    }

    public parse(document: TextDocument) {
        const tree = this.parser.parse(document.getText())
        this.uriToTSTree[document.uri] = tree;
    }
}
