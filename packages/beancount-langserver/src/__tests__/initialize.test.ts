import { initializeLspServer } from './mocks'

describe('lsp server - initialize', () => {
    it('initializes and responds to capabilities', async () => {
        const { server } = await initializeLspServer();
        expect(server.capabilities()).toMatchSnapshot()
    });

    it('registers lsp connection', async () => {
        const { connection, server } = await initializeLspServer();
        server.register(connection);

        expect(connection.onDidSaveTextDocument).toHaveBeenCalled();
        expect(connection.onDidOpenTextDocument).toHaveBeenCalled();
        expect(connection.onDocumentFormatting).toHaveBeenCalled();

    });
})
