import itertools
import logging
from typing import Dict, List, Optional, Union

from pygls.features import (
    COMPLETION,
    DEFINITION,
    DOCUMENT_HIGHLIGHT,
    DOCUMENT_SYMBOL,
    HOVER,
    INITIALIZE,
    REFERENCES,
    RENAME,
    SIGNATURE_HELP,
    TEXT_DOCUMENT_DID_CHANGE,
    TEXT_DOCUMENT_DID_OPEN,
    TEXT_DOCUMENT_DID_SAVE,
    TEXT_DOCUMENT_WILL_SAVE,
    WORKSPACE_SYMBOL,
    WORKSPACE_DID_CHANGE_WATCHED_FILES
)
from pygls.protocol import LanguageServerProtocol
from pygls.server import LanguageServer
from pygls.types import (
    CompletionItem,
    CompletionList,
    CompletionParams,
    Diagnostic,
    DidChangeTextDocumentParams,
    DidOpenTextDocumentParams,
    DidSaveTextDocumentParams,
    DocumentHighlight,
    DocumentSymbol,
    DocumentSymbolParams,
    Hover,
    InitializeParams,
    InitializeResult,
    Location,
    MarkupContent,
    ParameterInformation,
    Position,
    Range,
    RenameParams,
    SaveOptions,
    SignatureHelp,
    SignatureInformation,
    SymbolInformation,
    TextDocumentPositionParams,
    TextDocumentSyncKind,
    TextDocumentSyncOptions,
    TextEdit,
    WillSaveTextDocumentParams,
    WorkspaceEdit,
    WorkspaceSymbolParams,
)

from beancount import loader

class BeancountLanguageServerProtocol(LanguageServerProtocol):

    def bf_initialize(self, params: InitializeParams) -> InitializeResult:
        result :InitializeResult = super().bf_initialize(params)

        # pygls does not support TextDocumentSyncOptions that neovim lsp needs, hack it in
        result.capabilities.textDocumentSync = TextDocumentSyncOptions(True,TextDocumentSyncKind.INCREMENTAL,False,False,SaveOptions(True))

        return result

class BeancountLanguageServer(LanguageServer):
    """
    Beancount Language Server
    """
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.logger = logging.getLogger()
        self.diagnostics = {}

SERVER = BeancountLanguageServer(protocol_cls=BeancountLanguageServerProtocol)

def _validate(server: BeancountLanguageServer, params):

    server.show_message_log('Validating beancount ...')

    text_doc = server.workspace.get_document(params.textDocument.uri)

    source = text_doc.source

    keys_to_remove = []
    for filename in server.diagnostics:
        if len(server.diagnostics[filename]) == 0:
            keys_to_remove.append(filename)
        else:
            server.diagnostics[filename] = []

    for key in keys_to_remove:
        del server.diagnostics[key]

    entries, errors, options = loader.load_file(server._journal)
    for e in errors:
        server.logger.info(e)
        line = e.source['lineno']
        msg = e.message
        filename = e.source['filename']
        d = Diagnostic(
            Range(
                Position(line-1,0),
                Position(line-1,1)
            ),
            msg,
            source=filename
        )
        if filename not in server.diagnostics:
            server.diagnostics[filename] = []
        server.diagnostics[filename].append(d)


    for filename in server.diagnostics:
        server.publish_diagnostics(f"file://{filename}", server.diagnostics[filename])

    server.show_message_log('Done Validating')


@SERVER.feature(INITIALIZE)
def initialize(server:BeancountLanguageServer, params: InitializeParams):
    opts = params.initializationOptions
    server.logger.info(opts)
    server._journal = opts.journal

@SERVER.feature(TEXT_DOCUMENT_DID_SAVE)
def did_save(server: BeancountLanguageServer, params: DidSaveTextDocumentParams):
    """Actions run on textDocument/didSave"""
    _validate(server, params)

@SERVER.feature(TEXT_DOCUMENT_DID_OPEN)
def did_open(server: BeancountLanguageServer, params: DidOpenTextDocumentParams):
    """Actions run on textDocument/didOpen"""
    _validate(server, params)
