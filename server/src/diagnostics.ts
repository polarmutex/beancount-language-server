import * as LSP from 'vscode-languageserver'

interface BeancountErrorType {
    file: string;
    line: number;
    message: string;
}

interface BeancountFlagType {
    file: string;
    line: number
    message: string;
    flag: string;
}

export type DiagnosticMap = {
    [key: string]: LSP.Diagnostic[]
}

export function provideDiagnostics(errorsStr: string, flaggedStr: string): DiagnosticMap {
    const errors: BeancountErrorType[] = JSON.parse(errorsStr);
    const flagged: BeancountFlagType[] = JSON.parse(flaggedStr);

    const diagnostics: DiagnosticMap = {}

    errors.forEach(error => {
        const range = LSP.Range.create(
            LSP.Position.create(Math.max(error.line - 1, 0), 0),
            LSP.Position.create(Math.max(error.line, 1), 0),
        );
        const diagnostic = LSP.Diagnostic.create(
            range,
            error.message,
            LSP.DiagnosticSeverity.Error
        );
        diagnostic.source = "Beancount";
        if (diagnostics[error.file] === undefined) {
            diagnostics[error.file] = []
        }
        diagnostics[error.file].push(diagnostic);
    });

    flagged.forEach(entry => {
        const range = LSP.Range.create(
            LSP.Position.create(Math.max(entry.line - 1, 0), 0),
            LSP.Position.create(Math.max(entry.line, 1), 0),
        );
        const diagnostic = LSP.Diagnostic.create(
            range,
            entry.message,
            LSP.DiagnosticSeverity.Warning
        );
        diagnostic.source = "Beancount";
        if (diagnostics[entry.file] === undefined) {
            diagnostics[entry.file] = []
        }
        diagnostics[entry.file].push(diagnostic);
    });

    return diagnostics;
}
