'use strict';
var __awaiter = (this && this.__awaiter) || function (thisArg, _arguments, P, generator) {
    function adopt(value) { return value instanceof P ? value : new P(function (resolve) { resolve(value); }); }
    return new (P || (P = Promise))(function (resolve, reject) {
        function fulfilled(value) { try { step(generator.next(value)); } catch (e) { reject(e); } }
        function rejected(value) { try { step(generator["throw"](value)); } catch (e) { reject(e); } }
        function step(result) { result.done ? resolve(result.value) : adopt(result.value).then(fulfilled, rejected); }
        step((generator = generator.apply(thisArg, _arguments || [])).next());
    });
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.listen = void 0;
const LSP = require("vscode-languageserver");
const server_1 = require("./server");
const pkg = require('../package');
function listen() {
    // Create a connection for the server.
    // The connection uses stdin/stdout for communication.
    const connection = LSP.createConnection(new LSP.StreamMessageReader(process.stdin), new LSP.StreamMessageWriter(process.stdout));
    connection.onInitialize((params) => __awaiter(this, void 0, void 0, function* () {
        connection.console.log(`Initialized server v. ${pkg.version} for ${params.rootUri}`);
        const server = yield server_1.default.initialize(connection, params);
        server.register(connection);
        return {
            capabilities: server.capabilities(),
        };
    }));
    connection.listen();
}
exports.listen = listen;
//# sourceMappingURL=index.js.map