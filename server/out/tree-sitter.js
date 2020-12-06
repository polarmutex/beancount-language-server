"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
class TreeSitterAnalyzer {
    constructor(parser) {
        this.uriToTSTree = {};
        this.parser = parser;
    }
    static fromBeancountFile(connection, rootBeancountFile, parser) {
        const analyzer = new TreeSitterAnalyzer(parser);
        // TODO parse from root file
        return analyzer;
    }
    getTree(uri) {
        return this.uriToTSTree[uri];
    }
    parse(document) {
        const tree = this.parser.parse(document.getText());
        this.uriToTSTree[document.uri] = tree;
    }
}
exports.default = TreeSitterAnalyzer;
//# sourceMappingURL=tree-sitter.js.map