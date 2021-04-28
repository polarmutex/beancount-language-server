import {
    Connection,
    DiagnosticSeverity,
    Range,
    Position,
} from 'vscode-languageserver'
import { container } from 'tsyringe';
import * as path from 'path'

import { IDiagnostic } from "./diagnosticsProvider";
import { Settings } from '../../utils/settings'
import { execCmdSync } from '../../utils/runExternalCmd'

interface BeanCheckMessageType {
    file: string;
    line: number;
    message: string;
}

export class BeanCheckDiagnostics {
    private connection: Connection;
    private settings: Settings;

    constructor() {
        this.connection = container.resolve("Connection");
        this.settings = container.resolve("Settings");
    }


    public createDiagnostics = async (
    ): Promise<Map<string, IDiagnostic[]>> => {
        return await this.checkForErrors().then(
            (issues) => {
                if (issues.length === 0) {
                    return new Map([["", []]])
                }
                else {
                    return this.issuesToDiagnosticMap(issues);
                }
            },
        );
    }

    private async checkForErrors(): Promise<BeanCheckMessageType[]> {
        const settings = this.settings.getClientSettings()
        const journalFile = settings.journalFile
        const python = settings.pythonPath

        const beanCheckPy = path.join(__dirname, '../../../python/bean_check.py');

        const argsBeanCheck = [
            beanCheckPy,
            journalFile,
        ];

        try {

            const { stdout } = execCmdSync(
                python,
                argsBeanCheck,
                this.connection
            );

            const output:string[] = stdout.split('\n', 3);
            const errorsString:string = output[0]
            const flaggedString:string = output[1]

            const lines: BeanCheckMessageType[] = [];

            const errors: BeanCheckMessageType[] = JSON.parse(errorsString);
            lines.push(...errors)
            const flagged: BeanCheckMessageType[] = JSON.parse(flaggedString);
            lines.push(...flagged)

            return lines;

        } catch(error) {
            return [];
        }
    }

    public issuesToDiagnosticMap(
        issues: BeanCheckMessageType[],
    ): Map<string, IDiagnostic[]> {
        const issueMap = new Map<string,IDiagnostic[]>()
        issues.forEach((issue) => {
            const uri = issue.file;
            const range = Range.create(
                Position.create(Math.max(issue.line - 1, 0), 0),
                Position.create(Math.max(issue.line, 1), 0),
            );
            const diagnostic: IDiagnostic = {
                source: "BeanCheck",
                range: range,
                message: issue.message,
                severity: (issue.message == "Flagged Entry") ? DiagnosticSeverity.Warning : DiagnosticSeverity.Error,
                data: {
                    uri: uri,
                    code: "beanCheck"
                }
            };
            const arr = issueMap.get(uri) ?? [];

            arr.push(diagnostic);
            issueMap.set(uri, arr);
        });
        return issueMap;
    }
}
