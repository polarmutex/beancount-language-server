import { initializeLspServer } from './mocks'
import { FixtureUri } from './fixtures'

describe('lsp server - initialize', () => {
    it('initializes and responds to capabilities', async () => {
        const { server } = await initializeLspServer(FixtureUri.EXAMPLE);
        expect(server.capabilities()).toMatchSnapshot()
    });

    it('registers lsp connection', async () => {
        const { connection, server } = await initializeLspServer(FixtureUri.EXAMPLE);
        server.register(connection);

        expect(connection.onDidSaveTextDocument).toHaveBeenCalled();
        expect(connection.onDidOpenTextDocument).toHaveBeenCalled();
        expect(connection.onDocumentFormatting).toHaveBeenCalled();

    });
})
