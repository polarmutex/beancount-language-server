import * as LSP from 'vscode-languageserver';
export declare type DiagnosticMap = {
    [key: string]: LSP.Diagnostic[];
};
export declare function provideDiagnostics(errorsStr: string, flaggedStr: string): DiagnosticMap;
