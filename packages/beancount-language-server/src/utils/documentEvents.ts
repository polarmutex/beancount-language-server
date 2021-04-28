import { EventEmitter } from "events";
import {
    DidChangeTextDocumentParams,
    DidCloseTextDocumentParams,
    DidOpenTextDocumentParams,
    DidSaveTextDocumentParams,
    Connection,
} from "vscode-languageserver";
import { injectable, container } from 'tsyringe'

type DidChangeCallback = (params: DidChangeTextDocumentParams) => void;
type DidCloseCallback = (params: DidCloseTextDocumentParams) => void;
type DidOpenCallback = (params: DidOpenTextDocumentParams) => void;
type DidSaveCallback = (params: DidSaveTextDocumentParams) => void;

export interface IDocumentEvents {
    on(event: "open", listener: DidOpenCallback): this;
    on(event: "close", listener: DidCloseCallback): this;
    on(event: "save", listener: DidSaveCallback): this;
    on(event: "change", listener: DidChangeCallback): this;
}

@injectable()
export class DocumentEvents extends EventEmitter implements IDocumentEvents {
    constructor() {
        const connection = container.resolve<Connection>("Connection");
        super();

        connection.onDidOpenTextDocument((e) => this.emit("open", e));
        connection.onDidCloseTextDocument((e) => this.emit("close", e));
        connection.onDidSaveTextDocument((e) => this.emit("save", e));
        connection.onDidChangeTextDocument((e) => this.emit("change", e));
    }
}
