import {
  DocumentSemanticTokensProvider,
  Range,
  Position,
  SemanticTokens,
  SemanticTokensBuilder,
  SemanticTokensLegend,
  TextDocument,
} from "vscode";
import Parser from "tree-sitter";
import BeancountLang from "tree-sitter-beancount";

const termMap = new Map<string, { type: string; modifiers?: string[] }>();

export function buildLegend() {
  termMap.set("date", { type: "number" });
  termMap.set("txn", { type: "property" });
  termMap.set("account", { type: "type" });
  termMap.set("amount", { type: "number" });
  termMap.set("incomplete_amount", { type: "number" });
  termMap.set("currency", { type: "property" });
  termMap.set("key", { type: "label" });
  termMap.set("string", { type: "string" });
  termMap.set("tag", { type: "constant" });
  termMap.set("comment", { type: "comment" });

  const tokens: string[] = [];
  const modifiers: string[] = [];
  termMap.forEach((t) => {
    if (!tokens.includes(t.type)) tokens.push(t.type);
    t.modifiers?.forEach((m) => {
      if (!modifiers.includes(m)) modifiers.push(m);
    });
  });
  // Construct semantic token legend
  return new SemanticTokensLegend(tokens, modifiers);
}

class AST {
  public parser: Parser;
  public initialized: boolean = false;

  constructor() {}

  async init() {
    this.parser = new Parser();
    const langObj = this.parser.setLanguage(BeancountLang);
    this.parser.setLanguage(langObj);
    this.initialized = true;
  }

  tree(text: string) {
    return this.parser.parse(text);
  }

  parse(tree: Parser.Tree) {
    const tokens: {
      token: string;
      range: Range;
    }[] = [];
    const stack: Parser.SyntaxNode[] = [];
    let node = tree.rootNode.firstChild;

    while (stack.length > 0 || node) {
      if (node) {
        stack.push(node);
        node = node.firstChild;
      } else {
        node = stack.pop();
        const type = node.type;

        if (
          type === "date" ||
          type === "txn" ||
          type === "account" ||
          type === "amount" ||
          type === "incomplete_amount" ||
          type === "currency" ||
          type === "key" ||
          type === "string" ||
          type === "tag" ||
          type === "comment"
        ) {
          tokens.push({
            token: type,
            range: new Range(
              new Position(node.startPosition.row, node.startPosition.column),
              new Position(node.endPosition.row, node.endPosition.column)
            ),
          });
        }
        node = node.nextSibling;
      }
    }

    return tokens;
  }
}

export class SemanticTokensProvider implements DocumentSemanticTokensProvider {
  readonly ast: AST = new AST();
  readonly trees: { [doc: string]: Parser.Tree } = {};
  readonly supportedTerms: string[] = [];
  readonly legend: SemanticTokensLegend;

  constructor(legend: SemanticTokensLegend) {
    this.legend = legend;
  }

  provideDocumentSemanticTokens(doc: TextDocument): SemanticTokens {
    //if (!this.ast.initialized) {
    //    await this.ast.init();
    //}
    const tree = this.ast.tree(doc.getText());
    const tokens = this.ast.parse(tree);
    this.trees[doc.uri.toString()] = tree;

    const builder = new SemanticTokensBuilder(this.legend);
    tokens.forEach((t) => {
      const type = termMap.get(t.token)!.type;
      const modifiers = termMap.get(t.token)!.modifiers;
      if (t.range.start.line === t.range.end.line)
        return builder.push(t.range, type, modifiers);

      let line = t.range.start.line;
      builder.push(
        new Range(t.range.start, doc.lineAt(line).range.end),
        type,
        modifiers
      );

      for (line = line + 1; line < t.range.end.line; line++)
        builder.push(doc.lineAt(line).range, type, modifiers);

      builder.push(
        new Range(doc.lineAt(line).range.start, t.range.end),
        type,
        modifiers
      );
    });
    return builder.build();
  }
}
