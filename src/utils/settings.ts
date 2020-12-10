import { Connection } from "vscode-languageserver";
import { injectable, container } from "tsyringe";
import * as os from 'os'

export interface IClientSettings {
    journalFile: string;
    pythonPath: string;
}

@injectable()
export class Settings {

    private clientSettings: IClientSettings = {
        journalFile: "",
        pythonPath: ""
    }
    private connection: Connection;

    constructor(
        config: IClientSettings
    ) {
        this.connection = container.resolve<Connection>("Connection");
        this.updateSettings(config);
    }

    public getClientSettings(): IClientSettings {
        return this.clientSettings;
    }

    private updateSettings(config: IClientSettings): void {
        this.clientSettings = { ...this.clientSettings, ...config };
        this.clientSettings.journalFile.replace("~", os.homedir)
    }
}
