import { container } from "tsyringe";
import { isDeepStrictEqual } from "util";
import {
    DocumentFormattingParams,
    Position,
    TextEdit,
} from "vscode-languageserver";
import { URI } from "vscode-uri";
import { DocumentFormattingProvider } from '../src/providers/documentFormattingProvider';
import { getCaretPositionFromSource } from './utils/sourceParser';
import { SourceTreeParser } from './utils/sourceTreeParser';
import { Forest } from '../src/forest'

class MockDocumentFormatingProvider extends DocumentFormattingProvider {
    public handleFormatting(params: DocumentFormattingParams): Promise<TextEdit[]> {
        return this.handleDocumentFormatting(params);
    }
}

function textEditEquals(a: TextEdit, b: TextEdit): boolean {
    return (
        (a.newText === b.newText) &&
        (a.range.start.line === b.range.start.line) &&
        (a.range.start.character === b.range.start.character) &&
        (a.range.end.line === b.range.end.line) &&
        (a.range.end.character === b.range.end.character)
    );
}

describe("documentFormattingProvider", () => {
    const treeParser = new SourceTreeParser();
    let docFormattingProvider: MockDocumentFormatingProvider;

    const debug = true // process.argv.find((arg) => arg === "--debug");

    async function testDocumentFormatting(
        source: string,
        expectedTextEdits: TextEdit[],
    ) {
        await treeParser.init();

        if (!docFormattingProvider) {
            docFormattingProvider = new MockDocumentFormatingProvider();
        }

        const srcUri = URI.file("test.beancount").toString();
        const forest = container.resolve<Forest>("Forest")

        const tree = treeParser.getTree(source)
        if (tree) {
            forest.setTree(
                srcUri,
                tree
            );
        }

        const textEdits =
            await docFormattingProvider.handleFormatting({
                textDocument: {
                    uri: srcUri
                },
                options: {
                    tabSize: 4,
                    insertSpaces: true,
                    trimTrailingWhitespace: true,
                    insertFinalNewline: false,
                    trimFinalNewlines: true
                }
            });

        const textEditsExist = expectedTextEdits.every((textEdit) =>
            textEdits.find((e: TextEdit) => textEditEquals(textEdit, e)),
        );

        if (debug && !textEditsExist) {
            console.log(
                `Expecting ${JSON.stringify(expectedTextEdits)}, got ${JSON.stringify(
                    textEdits,
                )}`,
            );
        }

        expect(textEditsExist).toBeTruthy();

    }

    it("test success", async () => {
        const source = `
* Section header
;; Accounts (comments)
2013-01-01 open Expenses:Restaurant
2013-01-01 open Assets:Cash
2014-03-02 * "Something"
    Expenses:Restaurant   50.02 USD
    Assets:Cash
2014-03-05 balance Assets:Cash -50.02 USD
2014-03-10 * "Something"
    Assets:Other   10 HOOL {500.23} USD ; Bla
    Assets:Cash

`
        const expectedTextEdits: TextEdit[] = []
        expectedTextEdits.push({
            newText: "       ",
            range: {
                start: {
                    line: 6,
                    character: 23
                },
                end: {
                    line: 6,
                    character: 23
                }
            }
        });
        expectedTextEdits.push({
            newText: " ",
            range: {
                start: {
                    line: 8,
                    character: 30
                },
                end: {
                    line: 8,
                    character: 30
                }
            }
        });
        expectedTextEdits.push({
            newText: "                 ",
            range: {
                start: {
                    line: 10,
                    character: 16
                },
                end: {
                    line: 10,
                    character: 16
                }
            }
        });
        await testDocumentFormatting(source, expectedTextEdits);
    })

    it.skip("test_align_posting_starts", async () => {
        const source = `
2014-03-01 * "Something"
    Expenses:Restaurant   50.01 USD
    Assets:Cash
2014-03-02 * "Something"
    Expenses:Restaurant    50.02 USD
    Assets:Cash
2014-03-03 * "Something"
    Expenses:Restaurant 50.03 USD
    Assets:Cash
`
        const expectedTextEdits: TextEdit[] = []
        expectedTextEdits.push({
            newText: "",
            range: {
                start: {
                    line: 2,
                    character: 24
                },
                end: {
                    line: 2,
                    character: 25
                }
            }
        });
        expectedTextEdits.push({
            newText: "",
            range: {
                start: {
                    line: 5,
                    character: 24
                },
                end: {
                    line: 5,
                    character: 26
                }
            }
        });
        expectedTextEdits.push({
            newText: " ",
            range: {
                start: {
                    line: 8,
                    character: 24
                },
                end: {
                    line: 8,
                    character: 24
                }
            }
        });
        await testDocumentFormatting(source, expectedTextEdits);
    })

    it.skip("test_open_only_issue80", async () => {
        const source = `
2015-07-16 open Assets:BoA:checking USD
`
        const result = `
2015-07-16 open Assets:BoA:checking USD
`
        const expectedTextEdits: TextEdit[] = []
        await testDocumentFormatting(source, expectedTextEdits);
    })

    it.skip("test_commas", async () => {
        const source = `
* Section header
;; Accounts (comments)
2013-01-01 open Expenses:Restaurant
2013-01-01 open Assets:Cash
2014-03-02 * "Something"
    Expenses:Restaurant   1,050.02 USD
    Assets:Cash
2014-03-05 balance   Assets:Cash  -1,050.02 USD
2014-03-10 * "Something"
    Assets:Other   10 HOOL {5,000.23} USD ; Bla
    Assets:Cash
`
        const result = `
 * Section header
;; Accounts (comments)
2013-01-01 open Expenses:Restaurant
2013-01-01 open Assets:Cash
2014-03-02 * "Something"
    Expenses:Restaurant              1,050.02 USD
    Assets:Cash
2014-03-05 balance   Assets:Cash  -1,050.02 USD
2014-03-10 * "Something"
    Assets:Other                           10 HOOL {5,000.23} USD ; Bla
    Assets:Cash
`
        const expectedTextEdits: TextEdit[] = []
        await testDocumentFormatting(source, expectedTextEdits);
    })

    it.skip("test_currency_issue146", async () => {
        const source = `
          1970-01-01 open Equity:Opening-balances
          1970-01-01 open Assets:Investments
          2014-03-31 * "opening"
            Assets:Investments                 1.23 FOO_BAR
            Equity:Opening-balances
`
        const reuslt = `
1970-01-01 open Equity:Opening-balances
1970-01-01 open Assets:Investments
2014-03-31 * "opening"
    Assets:Investments  1.23 FOO_BAR
    Equity:Opening-balances
`
        const expectedTextEdits: TextEdit[] = []
        await testDocumentFormatting(source, expectedTextEdits);
    })

    it.skip("test_fixed_width", async () => {
        const source = `
2016 - 08 - 01 open Expenses: Test
2016 - 08 - 01 open Assets: Test
2016 - 08 - 02 * "" ""
    Expenses: Test     10.00 USD
    Assets: Test
`
        const result = `
    2016 - 08 - 01 open Expenses: Test
    2016 - 08 - 01 open Assets: Test
    2016 - 08 - 02 * "" ""
        Expenses: Test                           10.00 USD
        Assets: Test
`
        const expectedTextEdits: TextEdit[] = []
        await testDocumentFormatting(source, expectedTextEdits);
    })

    it.skip("test_fixed_column", async () => {
        const source = `
2016 - 08 - 01 open Expenses: Test
2016 - 08 - 01 open Assets: Test
2016 - 08 - 01 balance Assets: Test  0.00 USD
2016 - 08 - 02 * "" ""
    Expenses: Test     10.00 USD
    Assets: Test
`
        const result = `
2016 - 08 - 01 open Expenses: Test
2016 - 08 - 01 open Assets: Test
2016 - 08 - 01 balance Assets: Test              0.00 USD
2016 - 08 - 02 * "" ""
    Expenses: Test                            10.00 USD
    Assets: Test
`
        const expectedTextEdits: TextEdit[] = []
        await testDocumentFormatting(source, expectedTextEdits);
    })

    it.skip("test_metadata_issue400", async () => {
        const source = `
2020 - 01 - 01 open Assets: Test
2020 - 11 - 10 * Test
    payment_amount: 20.00 EUR
    Assets: Test   10.00 EUR
    Assets: Test - 10.00 EUR
`
        const result = `
2020 - 01 - 01 open Assets: Test
2020 - 11 - 10 * Test
    payment_amount: 20.00 EUR
    Assets: Test                              10.00 EUR
    Assets: Test - 10.00 EUR
`
        const expectedTextEdits: TextEdit[] = []
        await testDocumentFormatting(source, expectedTextEdits);
    })

    // Eventually we will want to support arithmetic expressions.
    // It will require to invoke the expression parser because
    // expressions are not guaranteed to be surrounded by matching
    // parentheses.
    it.skip("test_arithmetic_expressions", async () => {
        const source = `
2016-08-01 open Expenses:Test
2016-08-01 open Assets:Test
2016-08-02 * "" ""
    Expenses:Test     10.0/2 USD
    Assets:Test
`
        const result = `
2016-08-01 open Expenses:Test
2016-08-01 open Assets:Test
2016-08-02 * "" ""
    Expenses:Test     10.0/2 USD
    Assets:Test
`
        const expectedTextEdits: TextEdit[] = []
        await testDocumentFormatting(source, expectedTextEdits);
    })
})
