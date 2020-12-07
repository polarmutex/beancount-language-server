import { FixtureFolder } from './fixtures'
import { initializeLspServer, getMockConnection } from './mocks'

describe('lsp server initializes', () => {
    it('initializes and responds to capabilities', async () => {
        const { server } = await initializeLspServer();
        expect(server.capabilities()).toMatchSnapshot()
    });
})
