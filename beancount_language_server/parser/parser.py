import logging
import os
from pathlib import Path
from typing import Any, List, Optional, Set

from beancount.loader import _load
from beancount_language_server.util import log_time
from tree_sitter import Language, Parser, Node


module_path = os.path.dirname(__file__)
Language.build_library(
    # Store the library in the 'build' dir
    f'{module_path}/../../build/tree-sitter-beancount.so',

    [
        f'{module_path}/../../vendor/tree-sitter-beancount'
    ]
)

BEANCOUNT_LANGUAGE = Language(f'{module_path}/../../build/tree-sitter-beancount.so', 'beancount')

beancount_parser = Parser()
beancount_parser.set_language(BEANCOUNT_LANGUAGE)


class Parser(object):
    def __init__(self, root_file, use_tree_sitter=False):
        self.root_file = None
        self.use_tree_sitter =  use_tree_sitter
        self.root_file = root_file
        self.is_encrypted = False
        self.logger = logging.getLogger()

        # Tree-Sitter Use Only
        self.seen_files = {}

    def open(self):

        bc_entries, bc_errors, bc_options = self._beancount_load_file()

        if self.is_encrypted or not self.use_tree_sitter:
            return bc_entries, bc_errors, bc_options

        ts_entries, ts_errors, ts_options = self._tree_sitter_load_file()

        for left, right in zip(bc_entries, ts_entries):
            if left != right:
                self.logger.info(f"MisMatch:\n{left}\n{right}")
                return_tree_sitter[1].append(BeancountError(None, msg, right))

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

            with log_time("Tree-Sitter Parse Files", self.logger):
                self._tree_sitter_parse_file(self.root_file)

            #with log_time("Tree-Sitter Post Process", self.logger):
            #    self._tree_sitter_parse_files()

        #entries, parse_errors, options_map = parse_file(self.root_file)
        #entries.sort(key=entry_sortkey)
        entries = []
        parse_errors = []
        options_map = []
        return entries, parse_errors, options_map

    def _tree_sitter_parse_file(self, filename:str):
        contents = Path(filename).read_bytes()
        return self._tree_sitter_parse_bytes(contents, filename)

    def _tree_sitter_parse_bytes(self, contents:bytes, filename:str = None):

        # a set of the loaded files to avoid include cycles. This should only
        # contain absoute and resolved paths


        with log_time(f"Parsing {filename}", self.logger):
            tree = beancount_parser.parse(contents)

        if filename:
            filename = str(Path(filename).resolve())
            self.seen_files[filename] = tree

        self._tree_sitter_recursive_parse(tree.root_node.children, filename, contents)

    def _tree_sitter_recursive_parse(self, nodes: List[Node],filename: Optional[str], contents:bytes):

        # check for include directives
        for node in nodes:
            if node.type == 'directive':
                if node.children[0].type == 'include':
                    tnode = node.children[0].children[1]
                    include_filename = contents[tnode.start_byte+1 : tnode.end_byte-1].decode()

                    include_expanded = sorted(Path(filename).parent.glob(include_filename))
                    for included in include_expanded:
                        include_name = str(Path(included).resolve())

                        if include_name in self.seen_files:
                            self.logger.warning(f"Duplicate include file: {include_filename}")
                            continue
                        included_contents = included.read_bytes()
                        with log_time(f"Parsing {include_filename}", self.logger):
                            tree = beancount_parser.parse(included_contents)
                        self.seen_files[include_filename] = tree
                        self._tree_sitter_recursive_parse(tree.root_node.children, include_filename, included_contents)


    def _tree_sitter_post_process(self, entries, parse_errors, options_map):
        entries, errors = run_transformations(entries, parse_errors, options_map, None)

        valid_errors =  validation.validate(entries, options_map, None, None)
        errors.extend(valid_errors)

        options_map["input_hash"] = compute_input_hash(options_map["include"])

        return entries, errors, options_map
