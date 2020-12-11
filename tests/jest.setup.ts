import "reflect-metadata";
import { container } from 'tsyringe';
import { Connection } from 'vscode-languageserver';
import { mockDeep } from 'jest-mock-extended';
import { Settings } from '../src/utils/settings';
import { DocumentEvents } from '../src/utils/documentEvents';
import { Forest } from '../src/forest'

container.register("Connection", { useValue: mockDeep<Connection>() });
container.register("Forest", { useValue: new Forest() });
container.register("Settings", {
    useValue: new Settings({
        journalFile: "",
        pythonPath: ""
    }),
});
container.register("ClientSettings", {
    useValue: {},
});
container.registerSingleton("DocumentEvents", DocumentEvents);
