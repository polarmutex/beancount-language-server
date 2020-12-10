#!/usr/bin/env node

import "reflect-metadata";
import { container } from "tsyringe"; //must be after reflect-metadata

import * as path from 'path'
import { Command } from 'commander'
import {
    Connection,
    InitializeParams,
    InitializeResult,
    ProposedFeatures
} from 'vscode-languageserver'
import { createConnection } from 'vscode-languageserver/node'
import Parser from 'web-tree-sitter'
import BeancountLspServer from './server'
import { Settings } from './utils/settings'
import { DocumentEvents } from './utils/documentEvents'
import { TextDocumentEvents } from './utils/textDocumentEvents'

const pkg = require('../package')

const version = require('../package.json').version

const program = new Command('beancount-langserver')
    .version(version)
    .option('--stdio', 'use stdio')
    .option('--node-ipc', 'use node-ipc')
    .parse(process.argv);

container.register<Connection>("Connection", {
    useValue: createConnection(ProposedFeatures.all),
});
container.registerSingleton<Parser>("Parser", Parser);
container.registerSingleton("DocumentEvents", DocumentEvents);
container.register("TextDocumentEvents", {
    useValue: new TextDocumentEvents(),
});

const connection = container.resolve<Connection>("Connection");

connection.onInitialize(
    async (
        params: InitializeParams,
        cancel,
        progress
    ): Promise<InitializeResult> => {

        connection.console.info(
            `initialized server v. ${pkg.version}`
        );

        await Parser.init();
        const absolute = path.join(__dirname, "../tree-sitter-beancount.wasm")
        const pathToWasm = path.relative(process.cwd(), absolute);
        connection.console.info(
            `Loading Beancount tree-sitter syntax from ${pathToWasm}`
        );
        const language = await Parser.Language.load(pathToWasm)
        container.resolve<Parser>("Parser").setLanguage(language);

        container.register("Settings", {
            useValue: new Settings(params.initializationOptions)
        });

        const server = new BeancountLspServer(params, progress);

        server.register();

        await server.init()


        return server.capabilities;
    },
);

connection.listen();

// Don't die on unhandled Promise rejections
process.on("unhandledRejection", (reason, p) => {
    connection.console.error(
        `Unhandled Rejection at: Promise ${p} reason:, ${reason}`,
    );
});
