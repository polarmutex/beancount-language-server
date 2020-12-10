import * as fs from 'fs'
import * as path from 'path'
import * as lsp from 'vscode-languageserver'

export const FixtureFolder = path.join(__dirname, './fixtures/')

function getTextDocumentItem(uri: string): lsp.TextDocumentItem {
    return {
        uri: 'foo',
        languageId: 'bar',
        version: 0,
        text: fs.readFileSync(uri.replace('file://', ''), 'utf8'),
    }
}

export const FixtureUri = {
    EXAMPLE: `file://${path.join(FixtureFolder, 'example.beancount')}`,
    SIMPLE_ERROR: `file://${path.join(FixtureFolder, 'simple_error.beancount')}`,
    SIMPLE_FLAG: `file://${path.join(FixtureFolder, 'simple_flag.beancount')}`,
    SIMPLE_INCLUDE: `file://${path.join(FixtureFolder, 'simple_include.beancount')}`,
}

export const FixtureTextDocumentItem = {
    EXAMPLE: getTextDocumentItem(FixtureUri.EXAMPLE),
    SIMPLE_ERROR: getTextDocumentItem(FixtureUri.SIMPLE_ERROR),
    SIMPLE_FLAG: getTextDocumentItem(FixtureUri.SIMPLE_FLAG),
    SIMPLE_INCLUDE: getTextDocumentItem(FixtureUri.SIMPLE_INCLUDE),
}
