import * as Parser from 'tree-sitter'
const Beancount = require('tree-sitter-beancount')

export function initializeParser(): Parser {
    const parser = new Parser();
    parser.setLanguage(Beancount);
    return parser;
}
