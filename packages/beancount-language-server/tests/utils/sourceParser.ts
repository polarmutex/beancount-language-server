import { Position } from 'vscode-languageserver'

export function getCaretPositionFromSource(
    source: string,
): {
    position: Position;
    newSources: { [K: string]: string };
    fileWithCaret: string;
} {
    const sources = getSourceFiles(source);

    let position: Position | undefined;
    let fileWithCaret = "";

    for (const fileName in sources) {
        sources[fileName] = sources[fileName]
            .split("\n")
            .map((s, line) => {
                const character = s.search("{-caret-}");

                if (character >= 0) {
                    position = { line, character };
                    fileWithCaret = fileName;
                }

                return s.replace("{-caret-}", "");
            })
            .join("\n");
    }

    if (!position) {
        fail();
    }

    return { newSources: sources, position, fileWithCaret };
}

export function getSourceFiles(source: string): { [K: string]: string } {
    const sources: { [K: string]: string } = {};
    let currentFile = "";
    const regex = /--@ ([a-zA-Z/]+.beancount)/;

    const x = regex.exec(source);

    if (x == null || x[1] === undefined) {
        sources["Main.beancount"] = source;
    } else {
        source.split("\n").forEach((s) => {
            const match = regex.exec(s);

            if (match !== null) {
                sources[match[1]] = "";
                currentFile = match[1];
            } else if (currentFile !== "") {
                sources[currentFile] = sources[currentFile] + s + "\n";
            }
        });
    }

    return sources;
}
