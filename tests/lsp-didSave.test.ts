import { initializeLspServer } from './mocks'
import { FixtureTextDocumentItem, FixtureUri } from './fixtures'

describe('lsp server - didSave', () => {
    it('save valid file', async () => {
        const { connection, server } = await initializeLspServer(FixtureUri.EXAMPLE);
        server.register(connection);

        await server.onDidSaveTextDocument({
            textDocument: FixtureTextDocumentItem.EXAMPLE
        });

        expect(connection.onDidSaveTextDocument).toHaveBeenCalled();
        expect(connection.sendDiagnostics).toHaveBeenCalledTimes(0);
    });

    it('save simple invalid file', async () => {
        const { connection, server } = await initializeLspServer(FixtureUri.SIMPLE_ERROR);
        server.register(connection);

        await server.onDidSaveTextDocument({
            textDocument: FixtureTextDocumentItem.SIMPLE_ERROR
        });

        expect(connection.onDidSaveTextDocument).toHaveBeenCalled();
        expect(connection.sendDiagnostics).toHaveBeenCalledTimes(1);
    });

    it('save simple flagged file', async () => {
        const { connection, server } = await initializeLspServer(FixtureUri.SIMPLE_FLAG);
        server.register(connection);

        await server.onDidSaveTextDocument({
            textDocument: FixtureTextDocumentItem.SIMPLE_FLAG
        });

        expect(connection.onDidSaveTextDocument).toHaveBeenCalled();
        expect(connection.sendDiagnostics).toHaveBeenCalledTimes(1);
    });
})
