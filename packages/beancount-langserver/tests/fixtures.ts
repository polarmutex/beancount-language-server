import * as fs from 'fs'
import * as path from 'path'
import * as lsp from 'vscode-languageserver'

export const FixtureFolder = path.join(__dirname, './fixtures/')

function getDocument(uri: string) {
    return lsp.TextDocument.create(
        'foo',
        'bar',
        0,
        fs.readFileSync(uri.replace('file://', ''), 'utf8'),
    )
}
