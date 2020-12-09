// TODO: node-tree-sitter does not properly work with jest
// we get a module injection error see issue
//import * as Parser from 'tree-sitter'
//const Beancount = require('tree-sitter-beancount')

import * as Parser from 'web-tree-sitter'

export async function initializeParser(): Promise<Parser> {
    await Parser.init()
    const parser = new Parser;

    const Beancount = await Parser.Language.load(`${__dirname}/../tree-sitter-beancount.wasm`)
    parser.setLanguage(Beancount);

    return parser;
}
