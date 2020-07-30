import collections
import logging
import os
from pathlib import Path
from typing import Any, List, Optional, Set

from beancount.core.data import entry_sortkey, Entries
from beancount.loader import _load
from beancount.loader import compute_input_hash
from beancount.loader import run_transformations
from beancount.ops import validation
from beancount.parser import booking
from beancount_language_server.util import log_time
from beancount_language_server.parser import nodes as handlers
from beancount_language_server.parser.state import ParserState
from tree_sitter import Language, Parser, Node


module_path = os.path.dirname(__file__)
print(f"{module_path}")
#Language.build_library(
#    # Store the library in the 'build' dir
#    f'{module_path}/../../build/tree-sitter-beancount.so',

#    [
#        f'{module_path}/../../vendor/tree-sitter-beancount'
#    ]
#)

BEANCOUNT_LANGUAGE = Language(f'{module_path}/../../tree-sitter-beancount.so', 'beancount')

beancount_parser = Parser()
beancount_parser.set_language(BEANCOUNT_LANGUAGE)

BeancountError = collections.namedtuple('BeancountError', 'source message entry')


class Parser(object):
    def __init__(self, root_file, use_tree_sitter=False):
        self.root_file = None
        self.use_tree_sitter =  use_tree_sitter
        self.root_file = root_file
        self.is_encrypted = False
        self.logger = logging.getLogger()

    def open(self):

        bc_entries, bc_errors, bc_options = self._beancount_load_file()

        if self.is_encrypted or not self.use_tree_sitter:
            return bc_entries, bc_errors, bc_options

        ts_entries, ts_errors, ts_options = self._tree_sitter_load_file()

        for left, right in zip(bc_entries, ts_entries):
            if left != right:
                msg = f"MisMatch:\n{left}\n{right}"
                #self.logger.warning(msg)
                #ts_errors.append(BeancountError(left.meta, msg, right))

        return ts_entries, ts_errors, ts_options


    def change(self):
        pass

    def save(self):
        if not self.use_tree_sitter:
            return self._beancount_load_file()
        return None, None, None

    def _beancount_load_file(self):
        with log_time("Beancount Parser", self.logger):
            entries, errors, options = _load([(self.root_file, True)], None, None, None)
        return entries, errors, options

    def _tree_sitter_load_file(self):

        with log_time("Tree-Sitter Parser", self.logger):
            entries, parse_errors, options_map = self._tree_sitter_parse_file(self.root_file)
            entries.sort(key=entry_sortkey)

            entries, balance_errors = booking.book(entries, options_map)
            parse_errors.extend(balance_errors)

            entries, errors = run_transformations(entries, parse_errors, options_map, None)

            valid_errors =  validation.validate(entries, options_map, None, None)
            errors.extend(valid_errors)

            options_map["input_hash"] = compute_input_hash(options_map["include"])

        return entries, errors, options_map

    def _tree_sitter_parse_file(self, filename:str):
        contents = Path(filename).read_bytes()
        return self._tree_sitter_parse_bytes(contents, filename)

    def _tree_sitter_parse_bytes(self, contents:bytes, filename:str = None):

        # the parser state.
        state = ParserState(contents, filename)

        # a set of the loaded files to avoid include cycles. This should only
        # contain absoute and resolved paths
        seen_files: Set[str] = set()

        if filename:
            filename = str(Path(filename).resolve())
            seen_files.add(filename)

        with log_time(f"Parsing {filename}", self.logger):
            tree = beancount_parser.parse(contents)

        entries = self._tree_sitter_recursive_parse(tree.root_node.children, state, filename, seen_files)

        state.finalize()
        state.options["include"] = sorted(seen_files)
        return entries, state.errors, state.options

    def _tree_sitter_recursive_parse(self, nodes: List[Node], state: ParserState, filename: Optional[str], seen_files: Set[str]):

        # check for include directives
        #for node in nodes:
        #    if node.type == 'directive':
        #        if node.children[0].type == 'include':
        #            tnode = node.children[0].children[1]
        #            include_filename = contents[tnode.start_byte+1 : tnode.end_byte-1].decode()

        #            include_expanded = sorted(Path(filename).parent.glob(include_filename))
        #            for included in include_expanded:
        #                include_name = str(Path(included).resolve())

        #                if include_name in self.seen_files:
        #                    self.logger.warning(f"Duplicate include file: {include_filename}")
        #                    continue
        #                included_contents = included.read_bytes()
        #                with log_time(f"Parsing {include_filename}", self.logger):
        #                    tree = beancount_parser.parse(included_contents)
        #                self.seen_files[include_filename] = tree
        #                self._tree_sitter_recursive_parse(tree.root_node.children, include_filename, included_contents)

        entries: Entries = []
        for node in nodes:
            try:
                res = state.handle_node(node)
                if res is not None:
                    entries.append(res)

            except handlers.SyntaxError:
                node_contents = state.contents[node.start_byte : node.end_byte].decode()
                state.error(node, f"Syntax Error:\n{node_contents}\n{node.sexp()}")
            except handlers.IncludeFound as incl:
                if filename is None:
                    state.error(node, "Cannot resolve include when parsing a string")
                    continue

                included_expanded = sorted(Path(filename).parent.glob(incl.filename))
                if not included_expanded:
                    state.error(node, "Include glob did not match any files")
                for included in included_expanded:
                    included_name = str(included.resolve())
                    if included_name in seen_files:
                        state.error(node, f"Duplicate included file: {filename}")
                        continue
                    contents = included.read_bytes()
                    seen_files.add(included_name)
                    with log_time(f"parsing {included_name}", self.logger):
                        tree = beancount_parser.parse(contents)

                    # update state for the included file and recurse
                    with state.set_current_file(contents, included_name):
                        included_entries = self._tree_sitter_recursive_parse(tree.root_node.children, state, included_name, seen_files)
                        entries.extend(included_entries)
        return entries


    def _tree_sitter_post_process(self, entries, parse_errors, options_map):

        return entries, errors, options_map
