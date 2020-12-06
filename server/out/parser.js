"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.initializeParser = void 0;
const Parser = require("tree-sitter");
const Beancount = require('tree-sitter-beancount');
function initializeParser() {
    const parser = new Parser();
    parser.setLanguage(Beancount);
    return parser;
}
exports.initializeParser = initializeParser;
//# sourceMappingURL=parser.js.map