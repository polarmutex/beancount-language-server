import { Parser, Query, Tree } from 'web-tree-sitter';
import * as lsp from 'vscode-languageserver'
import { TextDocument } from 'vscode-languageserver-textdocument'
import * as fs from 'fs'
import * as path from 'path'
import { promisify } from 'util'
const readFileAsync = promisify(fs.readFile)
export default class TreeSitterAnalyzer {

    public static async fromBeancountFile(
        connection: lsp.Connection,
        rootBeancountFile: string,
        parser: Parser
    ): Promise<TreeSitterAnalyzer> {
        const analyzer = new TreeSitterAnalyzer(parser);

        const root_uri = `file://${rootBeancountFile}`

        const seen_files: string[] = []
        seen_files.push(root_uri)

        for (var i = 0; i < seen_files.length; i++) {
            const file_uri = seen_files[i];
            const file = file_uri.replace("file://", "")
            connection.console.log(file)
            connection.console.log(file_uri)
            const textDocument = TextDocument.create(
                file_uri,
                'beancount',
                1,
                await readFileAsync(file, 'utf8')
            );

            connection.console.log("Parsing ... " + file)

            analyzer.analyze(file_uri, textDocument)

            const include_files = analyzer.includeFileQuery
                .matches(analyzer.getTree(file_uri).rootNode)
                .map((match) => match.captures[0].node.text.replace(/"/g, ""));

            include_files.forEach((file) => {
                const include_file = path.join(path.dirname(file_uri.replace("file://", "")), file)
                const include_uri = `file://${include_file}`
                if (!seen_files.includes(include_uri)) {
                    seen_files.push(include_uri)
                }
            });
        }

        return analyzer;
    }

    private parser: Parser;
    private uriToTSTree: { [uri: string]: Tree } = {}
    private readonly includeFileQuery: Query;

    public constructor(parser: Parser) {
        this.parser = parser

        this.includeFileQuery = this.parser.getLanguage().query(
            `
            (include (string) @include_file )
        `
        )
    }

    public getTree(uri: string): Tree {
        return this.uriToTSTree[uri];
    }

    public analyze(uri: string, document: TextDocument) {
        const tree = this.parser.parse(document.getText())
        this.uriToTSTree[document.uri] = tree;
    }
}
