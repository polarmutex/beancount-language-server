import datetime
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
    CompletionItemKind,
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
from beancount.core.getters import get_accounts, get_all_payees, get_all_tags

class BeancountLanguageServerProtocol(LanguageServerProtocol):

    def bf_initialize(self, params: InitializeParams) -> InitializeResult:
        result :InitializeResult = super().bf_initialize(params)

        # pygls does not support TextDocumentSyncOptions that neovim lsp needs, hack it in
        result.capabilities.textDocumentSync = TextDocumentSyncOptions(True,TextDocumentSyncKind.INCREMENTAL,False,False,SaveOptions(include_text=False))

        return result

class BeancountLanguageServer(LanguageServer):
    """
    Beancount Language Server
    """
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.logger = logging.getLogger()
        self.diagnostics = {}
        self.accounts = []
        self.payees = []
        self.tags = []

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

    server.accounts = get_accounts(entries)
    server.payees = get_all_payees(entries)
    server.tags = get_all_tags(entries)


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

@SERVER.feature(COMPLETION, trigger_characters=["^",'"'])
def completion(server: BeancountLanguageServer, params: CompletionParams) -> CompletionList:
    """Returns completion items."""

    position = params.position
    document = server.workspace.get_document(params.textDocument.uri)
    word = document.word_at_position(position)
    trigger_char = document.lines[position.line][position.character]

    completion_items = []

    server.logger.debug(f"trigger_char: {trigger_char} - word: {word} - {position.character}")

    # Match start of Date
    if word.startswith("2"):

        today = datetime.date.today()
        year = today.year
        month = f"{today.month}".zfill(2)
        day = f"{today.day}".zfill(2)

        # Todays Date
        completion_item = CompletionItem(
            f"{year}-{month}-{day}",
            kind=CompletionItemKind.Reference,
            detail="Todays date",
            preselect=True
        )
        completion_items.append(completion_item)

        # Current Month
        completion_item = CompletionItem(
            f"{year}-{month}-",
            kind=CompletionItemKind.Reference,
            detail="Current Month"
        )
        completion_items.append(completion_item)

        # Prev Month
        first_of_current_month = today.replace(day=1)
        last_day_of_prev_month = first_of_current_month - datetime.timedelta(days=1)
        month = f"{last_day_of_prev_month.month}".zfill(2)
        year = last_day_of_prev_month.year
        completion_item = CompletionItem(
            f"{year}-{month}-",
            kind=CompletionItemKind.Reference,
            detail="Previous Month"
        )
        completion_items.append(completion_item)

        # Next Month
        month = today.month - 1 + 1
        year = today.year + month // 12
        month = f"{month % 12 + 1}".zfill(2)
        completion_item = CompletionItem(
            f"{year}-{month}",
            kind=CompletionItemKind.Reference,
            detail="Next Month"
        )
        completion_items.append(completion_item)

    elif trigger_char == '"':
        for payee in server.payees:
            if word in payee:
                completion_item = CompletionItem(
                    payee,
                    kind=CompletionItemKind.Text,
                    detail="Beancount Payee",
                )
                completion_items.append(completion_item)

    else:
        for account in server.accounts:
            if word in account:
                completion_item = CompletionItem(
                    account,
                    kind=CompletionItemKind.Text,
                    detail="Beancount Account",
                )
                completion_items.append(completion_item)

    return CompletionList(
        is_incomplete=True,
        items=completion_items
    )
