import { container } from "tsyringe";
import { isDeepStrictEqual } from "util";
import {
    CompletionContext,
    CompletionItem,
    CompletionList,
    CompletionParams,
    Position,
    TextEdit,
} from "vscode-languageserver";
import { URI } from "vscode-uri";
import { CompletionProvider } from "../src/providers/completionProvider";
import { getCaretPositionFromSource } from "./utils/sourceParser";
import { SourceTreeParser } from "./utils/sourceTreeParser";
import { Forest } from '../src/forest'

class MockCompletionProvider extends CompletionProvider {
    public handleCompletion(params: CompletionParams): CompletionItem[] | CompletionList {
        return this.handleCompletionRequest(params);
    }
}

type exactCompletions = "exactMatch" | "partialMatch";
type dotCompletions = "triggeredByDot" | "normal";

describe("CompletionProvider", () => {
    const treeParser = new SourceTreeParser();

    const debug = true // process.argv.find((arg) => arg === "--debug");

    /**
        * Run completion tests on a source
    *
        * @param source The source code in an array of lines
    * @param expectedCompletions The array of expected completions
    * @param testExactCompletions Test that the completion list ONLY includes the expected completions
    * @param testDotCompletion Test completions if a dot was the trigger character
        */
    async function testCompletions(
        source: string,
        expectedCompletions: (string | CompletionItem)[],
        testExactCompletions: exactCompletions = "partialMatch",
        testDotCompletion: dotCompletions = "normal",
    ) {
        await treeParser.init();
        const completionProvider = new MockCompletionProvider();

        const { newSources, position, fileWithCaret } = getCaretPositionFromSource(source);

        if (!position) {
            throw new Error("Getting position failed");
        }

        const testUri = URI.file(fileWithCaret).toString();
        const forest = container.resolve<Forest>("Forest")
        for (var src in newSources) {
            const srcUri = URI.file(src).toString();
            const tree = treeParser.getTree(newSources[src])
            if (tree) {
                forest.setTree(
                    srcUri,
                    tree
                );
            }
        }
        const sourceFile = forest.getByUri(testUri);

        function testCompletionsWithContext(context: CompletionContext): void {
            if (!sourceFile) throw new Error("Getting tree failed");

            const completions =
                completionProvider.handleCompletion({
                    textDocument: { uri: testUri },
                    position: position!,
                    context
                }) ?? null;

            const completionsList = Array.isArray(completions)
                ? completions
                : completions.items;

            if (debug && completionsList.length === 0) {
                console.log(
                    `No completions found with context ${JSON.stringify(
                        context,
                    )}, expected completions: ${JSON.stringify(expectedCompletions)}`,
                );
            } else if (
                debug &&
                testExactCompletions === "exactMatch" &&
                completionsList.length !== expectedCompletions.length
            ) {
                console.log(
                    `Wrong completions: ${JSON.stringify(
                        completionsList.map((c) => c.label),
                    )}, expected: ${JSON.stringify(expectedCompletions)}`,
                );
            }

            if (testExactCompletions === "exactMatch") {
                expect(completionsList.length).toBe(expectedCompletions.length);
            } else {
                expect(completionsList.length).toBeGreaterThanOrEqual(
                    expectedCompletions.length,
                );
            }

            expectedCompletions.forEach((completion) => {
                const result = completionsList.find((c) => {
                    if (typeof completion === "string") {
                        return c.label === completion;
                    } else {
                        // Compare label, detail, and text edit text
                        return (
                            c.label === completion.label &&
                            c.detail === completion.detail &&
                            c.additionalTextEdits &&
                            completion.additionalTextEdits &&
                            isDeepStrictEqual(
                                c.additionalTextEdits[0],
                                completion.additionalTextEdits[0],
                            )
                        );
                    }
                });

                if (!result && debug) {
                    console.log(
                        `Could not find ${completion} in ${JSON.stringify(
                            completionsList,
                        )}`,
                    );
                }

                expect(result).toBeTruthy();
            });
        }

        testCompletionsWithContext({ triggerKind: 1 });

        if (testDotCompletion === "triggeredByDot") {
            testCompletionsWithContext({ triggerKind: 2, triggerCharacter: "." });
        }
    }

    it("Should complete date on empty line", async () => {
        const source = `
--@ Test.beancount
2020-12-01 open Assets:Checking
{-caret-}
        `;

        const d: Date = new Date();

        const currentYear = d.getFullYear()
        const currentMonth = (d.getMonth() + 1).toString().padStart(2, "0")
        const prevMonth = (d.getMonth()).toString().padStart(2, "0")
        const currentDate = (d.getDate()).toString().padStart(2, "0")

        await testCompletions(source, [`${currentYear}-${currentMonth}-${currentDate}`])
    })

    it("Should complete payee str 1", async () => {
        const source = `
--@ Test.beancount
2020-12-01 txn "Foo" "Bar"
    Assets:Checking    5.00
    Expenses:Food
2020-12-01 txn "{-caret-}
`;

        await testCompletions(source, ["Foo"])
    })

    it("Should complete payee str 2", async () => {
        const source = `
--@ Test.beancount
2020-12-01 txn "Foo" "Bar"
    Assets:Checking    5.00
    Expenses:Food
2020-12-01 txn "Test" "{-caret-}
`;
        await testCompletions(source, ["Bar"])
    })

    it("Should complete accounts", async () => {
        const source = `
--@ Test.beancount
2020-12-01 open Assets:Checking

2020-12-01 txn "Test"
    As{-caret-}

`;
        await testCompletions(source, ["Assets:Checking"])
    })
})
