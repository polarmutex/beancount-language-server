import {
    Connection,
    Diagnostic,
} from 'vscode-languageserver'
import {
    TextDocument,
} from 'vscode-languageserver-textdocument'
import { container, injectable } from 'tsyringe';

import { TextDocumentEvents } from '../../utils/textDocumentEvents'
import { Settings } from '../../utils/settings'
import { BeanCheckDiagnostics } from './beanCheckDiagnostics'
//import { TreeSitterDiagnostics } from './treesitterDiagnostics'
import { DiagnosticKind, BeancountDiagnostics } from './beancountDiagnostics'

export type DiagnosticSource = "BeanCheck" | "TreeSitter";

export interface IDiagnostic extends Diagnostic {
  source: DiagnosticSource;
  data: {
    uri: string;
    code: string;
  };
}

@injectable()
export class DiagnosticsProvider {
    private connection: Connection;
    private documentEvents: TextDocumentEvents;
    private settings: Settings;
    private beanCheckDiagnostics: BeanCheckDiagnostics;
    //private treesitterDiagnostics: TreeSitterDiagnostics;
    private currentDiagnostics: Map<string, BeancountDiagnostics>;

    constructor() {
        this.connection = container.resolve("Connection");
        this.documentEvents = container.resolve("TextDocumentEvents");
        this.settings = container.resolve("Settings");
        this.beanCheckDiagnostics = container.resolve(BeanCheckDiagnostics);
        //this.treesitterDiagnostics = container.resolve(TreeSitterDiagnostics);

        this.currentDiagnostics = new Map<string, BeancountDiagnostics>();

        const handleSaveOrOpen = (doc: { document: TextDocument }): void => {
            this.getBeanCheckDiagnostics();
            // If we disable tree-sitter diagnostics on change we need to call it here
        }
        this.documentEvents.on("open", handleSaveOrOpen)
        this.documentEvents.on("save", handleSaveOrOpen)

    }

    private async getBeanCheckDiagnostics(): Promise<void> {
        const checkDiagnostics = await this.beanCheckDiagnostics.createDiagnostics();

        this.resetDiagnostics(checkDiagnostics, DiagnosticKind.BeanCheck);

        if (checkDiagnostics.size == 1) {
            return
        }

        checkDiagnostics.forEach((diagnostics, diagnosticsUri) => {
            this.updateDiagnostics(diagnosticsUri, DiagnosticKind.BeanCheck, diagnostics);
        });


        this.currentDiagnostics.forEach((_, uri) => {
            if (!checkDiagnostics.has(uri)) {
                this.updateDiagnostics(uri, DiagnosticKind.BeanCheck, []);
            }
        });
    }

    private updateDiagnostics(
        uri: string,
        kind: DiagnosticKind,
        diagnostics: IDiagnostic[],
    ): void {
        let didUpdate = false;
        let fileDiagnostics = this.currentDiagnostics.get(uri);

        if(fileDiagnostics) {
            didUpdate = fileDiagnostics.update(kind, diagnostics);
        }
        else if(diagnostics.length > 0) {
            fileDiagnostics = new BeancountDiagnostics(uri);
            fileDiagnostics.update(kind, diagnostics);
            this.currentDiagnostics.set(uri, fileDiagnostics);
            didUpdate = true;
        }

        if (didUpdate) {
            const fileDiagnostics = this.currentDiagnostics.get(uri);
            if(fileDiagnostics) {
                this.connection.sendDiagnostics({
                    uri: "file://" + uri,
                        diagnostics: fileDiagnostics ? fileDiagnostics.get() : [],
                });
            }
        }
    }

    // clear out previous diagnostics from prior runs
    private resetDiagnostics(
        diagnosticList: Map<string, Diagnostic[]>,
        diagnosticKind: DiagnosticKind,
    ): void {
        this.currentDiagnostics.forEach((fileDiagnostics, diagnosticsUri) => {
            if(
                !diagnosticList.has(diagnosticsUri) &&
                fileDiagnostics.getForKind(diagnosticKind).length > 0
            ) {
                diagnosticList.set(diagnosticsUri, []);
            }
        });
    }
}
