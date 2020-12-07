import * as lsp from 'vscode-languageserver'
import BeancountLspServer from './lsp-server'

const pkg = require('../package')

export function createLspConnection(): lsp.IConnection {
    const connection = lsp.createConnection();

    connection.onInitialize(
        async (params: lsp.InitializeParams): Promise<lsp.InitializeResult> => {
            connection.console.log(`initialized server v. ${pkg.version}`);
            const server = await BeancountLspServer.initialize(connection, params);

            server.register(connection);

            return {
                capabilities: server.capabilities()
            }
        },
    );

    return connection;
}
