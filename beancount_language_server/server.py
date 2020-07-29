import datetime
import itertools
import logging
import re
import os
from typing import Dict, List, Optional, Union

from pygls.features import (
    COMPLETION,
    DEFINITION,
    DOCUMENT_HIGHLIGHT,
    DOCUMENT_SYMBOL,
    FORMATTING,
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
    CompletionTriggerKind,
    Diagnostic,
    DidChangeTextDocumentParams,
    DidOpenTextDocumentParams,
    DidSaveTextDocumentParams,
    DocumentFormattingParams,
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
from beancount.scripts.format import align_beancount

from beancount_language_server.parser.parser import Parser

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

        self.use_tree_sitter = False
        self.parser = None

    def _publish_beancount_diagnostics(self, params, errors):

        text_doc = self.workspace.get_document(params.textDocument.uri)
        source = text_doc.source

        keys_to_remove = []
        for filename in self.diagnostics:
            if len(self.diagnostics[filename]) == 0:
                keys_to_remove.append(filename)
            else:
                self.diagnostics[filename] = []

        for key in keys_to_remove:
            del self.diagnostics[key]

        for e in errors:
            self.logger.info(e)
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
            if filename not in self.diagnostics:
                self.diagnostics[filename] = []
            self.diagnostics[filename].append(d)

        for filename in self.diagnostics:
            self.publish_diagnostics(f"file://{filename}", self.diagnostics[filename])

        #self.accounts = get_accounts(entries)
        #self.payees = get_all_payees(entries)
        #self.tags = get_all_tags(entries)


SERVER = BeancountLanguageServer(protocol_cls=BeancountLanguageServerProtocol)


@SERVER.feature(INITIALIZE)
def initialize(server:BeancountLanguageServer, params: InitializeParams):
    opts = params.initializationOptions
    server.logger.info(opts)
    server._journal = os.path.expanduser(opts.journal)
    server.use_tree_sitter = opts.use_tree_sitter
    server.parser = Parser(server._journal, server.use_tree_sitter)

@SERVER.feature(TEXT_DOCUMENT_DID_SAVE)
def did_save(server: BeancountLanguageServer, params: DidSaveTextDocumentParams):
    """Actions run on textDocument/didSave"""
    entries, errors, options = server.parser.save()
    server._publish_beancount_diagnostics(params, errors)

@SERVER.feature(TEXT_DOCUMENT_DID_OPEN)
def did_open(server: BeancountLanguageServer, params: DidOpenTextDocumentParams):
    """Actions run on textDocument/didOpen"""
    entries, errors, options = server.parser.open()
    server._publish_beancount_diagnostics(params, errors)

@SERVER.feature(COMPLETION, trigger_characters=["^",'"'])
def completion(server: BeancountLanguageServer, params: CompletionParams) -> CompletionList:
    """Returns completion items."""

    position = params.position
    document = server.workspace.get_document(params.textDocument.uri)
    word = document.word_at_position(position)

    if (hasattr(params, 'context') and params.context.triggerKind == CompletionTriggerKind.TriggerCharacter):
        trigger_char = params.context.triggerCharacter
    else:
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

@SERVER.feature(FORMATTING)
def formatting(server: BeancountLanguageServer, params: DocumentFormattingParams):
    document = server.workspace.get_document(params.textDocument.uri)

    content = document.source

    result = align_beancount(content) # format_beancount(content)

    lines = content.count('\n')
    return [
        TextEdit(Range(Position(0, 0), Position(lines + 1, 0)),
        result)
    ]

