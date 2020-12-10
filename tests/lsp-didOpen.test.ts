import { initializeLspServer } from './mocks'
import { FixtureTextDocumentItem, FixtureUri } from './fixtures'
describe('lsp server - didOpen', () => {
    it('calls didOpen', async () => {
        const { connection, server } = await initializeLspServer(FixtureUri.EXAMPLE);
        server.register(connection);

        await server.onDidOpenTextDocument({
            textDocument: FixtureTextDocumentItem.EXAMPLE
        });

        expect(connection.onDidOpenTextDocument).toHaveBeenCalled();
        expect(connection.sendDiagnostics).toHaveBeenCalledTimes(0);
    });

    it('calls didOpen and handles multiple imports', async () => {
        const { connection, server } = await initializeLspServer(FixtureUri.SIMPLE_INCLUDE);
        server.register(connection);

        await server.onDidOpenTextDocument({
            textDocument: FixtureTextDocumentItem.SIMPLE_INCLUDE
        });

        expect(connection.onDidOpenTextDocument).toHaveBeenCalled();
        expect(connection.sendDiagnostics).toHaveBeenCalledTimes(0);
    });
})

