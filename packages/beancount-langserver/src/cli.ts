import { Command } from 'commander'
import { createLspConnection } from './lsp-connection'

const version = require('../../../package.json').version

const program = new Command('beancount-langserver')
    .version(version)
    .parse(process.argv);

createLspConnection()


