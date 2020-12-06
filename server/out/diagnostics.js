"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.provideDiagnostics = void 0;
const LSP = require("vscode-languageserver");
function provideDiagnostics(errorsStr, flaggedStr) {
    const errors = JSON.parse(errorsStr);
    const flagged = JSON.parse(flaggedStr);
    const diagnostics = {};
    errors.forEach(error => {
        const range = LSP.Range.create(LSP.Position.create(Math.max(error.line - 1, 0), 0), LSP.Position.create(Math.max(error.line, 1), 0));
        const diagnostic = LSP.Diagnostic.create(range, error.message, LSP.DiagnosticSeverity.Error);
        diagnostic.source = "Beancount";
        if (diagnostics[error.file] === undefined) {
            diagnostics[error.file] = [];
        }
        diagnostics[error.file].push(diagnostic);
    });
    flagged.forEach(entry => {
        const range = LSP.Range.create(LSP.Position.create(Math.max(entry.line - 1, 0), 0), LSP.Position.create(Math.max(entry.line, 1), 0));
        const diagnostic = LSP.Diagnostic.create(range, entry.message, LSP.DiagnosticSeverity.Warning);
        diagnostic.source = "Beancount";
        if (diagnostics[entry.file] === undefined) {
            diagnostics[entry.file] = [];
        }
        diagnostics[entry.file].push(diagnostic);
    });
    return diagnostics;
}
exports.provideDiagnostics = provideDiagnostics;
//# sourceMappingURL=diagnostics.js.map