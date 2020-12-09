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
})

