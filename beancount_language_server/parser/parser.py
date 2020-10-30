import collections
import functools
import inspect
import io
import logging
import os
from pathlib import Path
import textwrap
from typing import Any, List, Optional, Set
from pkg_resources import resource_filename
from importlib.machinery import EXTENSION_SUFFIXES

from beancount.core import data
from beancount.core.number import MISSING
from beancount.loader import _load
from beancount.loader import compute_input_hash
from beancount.loader import run_transformations
from beancount.ops import validation
from beancount.parser import booking
from beancount.parser.parser import parse_string as beancount_parser_string
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

EXT = EXTENSION_SUFFIXES[-1]
BEANCOUNT_LANGUAGE = Language(
        resource_filename("beancount_language_server", "tree_sitter_beancount" + EXT), 'beancount')

beancount_parser = Parser()
beancount_parser.set_language(BEANCOUNT_LANGUAGE)

BeancountError = collections.namedtuple('BeancountError', 'source message entry')

logger = logging.getLogger()

class Parser(object):
    def __init__(self, root_file, use_tree_sitter=False):
        self.root_file = None
        self.use_tree_sitter =  use_tree_sitter
        self.root_file = root_file
        self.is_encrypted = False

    def open(self):

        bc_entries, bc_errors, bc_options = self._beancount_load_file()

        if self.is_encrypted or not self.use_tree_sitter:
            return bc_entries, bc_errors, bc_options

        ts_entries, ts_errors, ts_options = self._tree_sitter_load_file()

        for left, right in zip(bc_entries, ts_entries):
            if left != right:
                msg = f"MisMatch:\n{left}\n{right}"
                #logger.warning(msg)
                ts_errors.append(BeancountError(left.meta, msg, right))

        return ts_entries, ts_errors, ts_options


    def change(self):
        pass

    def save(self):
        if not self.use_tree_sitter:
            return self._beancount_load_file()

        ts_entries, ts_errors, ts_options = self._tree_sitter_load_file()

        return ts_entries, ts_errors, ts_options

    def _beancount_load_file(self):
        with log_time("Beancount Parser", logger):
            entries, errors, options = _load([(self.root_file, True)], None, None, None)
        return entries, errors, options

    def _tree_sitter_load_file(self):

        with log_time("Tree-Sitter Parser", logger):
            entries, parse_errors, options_map = _tree_sitter_parse_file(self.root_file)
            entries.sort(key=data.entry_sortkey)

            entries, balance_errors = booking.book(entries, options_map)
            parse_errors.extend(balance_errors)

            entries, errors = run_transformations(entries, parse_errors, options_map, None)

            valid_errors =  validation.validate(entries, options_map, None, None)
            errors.extend(valid_errors)

            options_map["input_hash"] = compute_input_hash(options_map["include"])

        return entries, errors, options_map


def _tree_sitter_parse_file(filename: str):
    contents = Path(filename).read_bytes()
    return _tree_sitter_parse_bytes(contents, filename)

def _tree_sitter_parse_string(contents: str, filename: str = None):
    return _tree_sitter_parse_bytes(contents.encode(), filename)

def _tree_sitter_parse_bytes(contents: bytes, filename: str = None):

    # the parser state.
    state = ParserState(contents, filename)

    # a set of the loaded files to avoid include cycles. This should only
    # contain absoute and resolved paths
    seen_files: Set[str] = set()

    if filename:
        filename = str(Path(filename).resolve())
        seen_files.add(filename)

    with log_time(f"Parsing {filename}", logger):
        tree = beancount_parser.parse(contents)

    entries = _tree_sitter_recursive_parse(tree.root_node.children, state, filename, seen_files)

    state.finalize()
    state.options["include"] = sorted(seen_files)
    return entries, state.errors, state.options

def _tree_sitter_recursive_parse(nodes: List[Node], state: ParserState, filename: Optional[str], seen_files: Set[str]):

    # check for include directives
    #for node in nodes:
    #    if node.type == 'directive':
    #        if node.children[0].type == 'include':
    #            tnode = node.children[0].children[1]
    #            include_filename = contents[tnode.start_byte+1 : tnode.end_byte-1].decode()

    #            include_expanded = sorted(Path(filename).parent.glob(include_filename))
    #            for included in include_expanded:
    #                include_name = str(Path(included).resolve())

    #                if include_name in seen_files:
    #                    logger.warning(f"Duplicate include file: {include_filename}")
    #                    continue
    #                included_contents = included.read_bytes()
    #                with log_time(f"Parsing {include_filename}", logger):
    #                    tree = beancount_parser.parse(included_contents)
    #                .seen_files[include_filename] = tree
    #                _tree_sitter_recursive_parse(tree.root_node.children, include_filename, included_contents)

    entries: data.Entries = []
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
                with log_time(f"parsing {included_name}", logger):
                    tree = beancount_parser.parse(contents)

                # update state for the included file and recurse
                with state.set_current_file(contents, included_name):
                    included_entries = _tree_sitter_recursive_parse(tree.root_node.children, state, included_name, seen_files)
                    entries.extend(included_entries)
    return entries


def tree_sitter_parse_doc(expect_errors=False, allow_incomplete=False):
    """Factory of decorators that parse the function's docstring as an argument.
    Note that the decorators thus generated only run the parser on the tests,
    not the loader, so is no validation, balance checks, nor plugins applied to
    the parsed text.
    Args:
      expect_errors: A boolean or None, with the following semantics,
        True: Expect errors and fail if there are none.
        False: Expect no errors and fail if there are some.
        None: Do nothing, no check.
      allow_incomplete: A boolean, if true, allow incomplete input. Otherwise
        barf if the input would require interpolation. The default value is set
        not to allow it because we want to minimize the features tests depend on.
    Returns:
      A decorator for test functions.
    """
    def decorator(fun):
        """A decorator that parses the function's docstring as an argument.
        Args:
          fun: the function object to be decorated.
        Returns:
          A decorated test function.
        """
        # filename = inspect.getfile(fun)
        # lines, lineno = inspect.getsourcelines(fun)

        # decorator line + function definition line (I realize this is largely
        # imperfect, but it's only for reporting in our tests) - empty first line
        # stripped away.
        # lineno += 1

        @functools.wraps(fun)
        def wrapper(self):
            assert fun.__doc__ is not None, ("You need to insert a docstring on {}".format(fun.__name__))
            # dedent doc string
            content = textwrap.dedent(fun.__doc__)
            entries, errors, options_map = _tree_sitter_parse_string(content)
            #entries, errors, options_map = beancount_parser_string(content)

            if not allow_incomplete and any(_is_entry_incomplete(entry)
                                            for entry in entries):
                self.fail("parse_doc() may not use interpolation.")

            if expect_errors is not None:
                if expect_errors is False and errors:
                    oss = io.StringIO()
                    # printer.print_errors(errors, file=oss)
                    self.fail("Unexpected errors found:\n{}".format(oss.getvalue()))
                elif expect_errors is True and not errors:
                    self.fail("Expected errors, none found:")

            return fun(self, entries, errors, options_map)

        wrapper.__input__ = wrapper.__doc__
        wrapper.__doc__ = None
        return wrapper

    return decorator

def _is_entry_incomplete(entry):
    """Detect the presence of elided amounts in Transactions.
    Args:
      entries: A directive.
    Returns:
      A boolean, true if there are some missing portions of any postings found.
    """
    if isinstance(entry, data.Transaction):
        if any(_is_posting_incomplete(posting) for posting in entry.postings):
            return True
    return False

def _is_posting_incomplete(posting):
    """Detect the presence of any elided amounts in a Posting.
    If any of the possible amounts are missing, this returns True.
    Args:
      entries: A directive.
    Returns:
      A boolean, true if there are some missing portions of any postings found.
    """
    units = posting.units
    if (units is MISSING or
        units.number is MISSING or
        units.currency is MISSING):
        return True
    price = posting.price
    if (price is MISSING or
        price is not None and (price.number is MISSING or
                               price.currency is MISSING)):
        return True
    cost = posting.cost
    if cost is not None and (cost.number_per is MISSING or
                             cost.number_total is MISSING or
                             cost.currency is MISSING):
        return True
    return False
