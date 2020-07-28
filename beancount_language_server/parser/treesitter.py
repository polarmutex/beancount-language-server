import logging
import os
from pathlib import Path
from typing import Set

from tree_sitter import Language, Parser

from beancount_language_server.parser.sttae import BaseState

logger = logging.getLogger()

logger.info("Setting up Beancoun Tree Sitter")

module_path = os.path.dirname(__file__)
Language.build_library(
    # Store the library in the 'build' dir
    f'{module_path}/../build/tree-sitter-beancount.so',

    [
        f'{module_path}/../vendor/tree-sitter-beancount'
    ]
)

BEANCOUNT_LANGUAGE = Language(f'{module_path}/../build/tree-sitter-beancount.so', 'beancount')

logger.info("Setting up Beancount Parser")
parser = Parser()
parser.set_language(BEANCOUNT_LANGUAGE)

def parse_file(filename:str):

    logger.info(f"Parsing {filename} ... ")
    contents = Path(filename).read_bytes()
    return parse_bytes(contents, filename)

def parse_string(contents:str, filename:str = None):
    return parse_bytes(contents.encode(), filename)

def parse_bytes(contents:bytes, filename:str = None):

    # a set of the loaded files to avoid include cycles. This should only
    # contain absoute and resolved paths
    seen_files: Set[str] = set()

    if filename:
        filename = str(Path(filename).resolve())
        seen_files.add(filename)

    # with log_time(f"Parsing {filename}", LOG):
    tree = parser.parse(contents)

    entries = _recursive_parse(tree.root_node.children, filename, seen_files)

    return entries, state

class ParserState(BaseState):
    """The state of the parser.
    This is where data that needs to be kept in the state lives.
    """

    def finalize(self) -> None:
        """Check for unbalanced tags and metadata."""
        for tag in self.tags:
            self.error(None, f"Unbalanced pushed tag: '{tag}'")

        for key, value_list in self.meta.items():
            self.error(
                None,
                f"Unbalanced metadata key '{key}'; "
                f"leftover metadata '{str(value_list)}'",
            )

    def dcupdate(self, number, currency) -> None:
        """Update the display context.
        One or both of the arguments might be `MISSING`, in which case we do
        nothing.
        Args:
            number: The number.
            currency: The currency.
        """
        if number is not MISSING and currency is not MISSING:
            self._dcupdate(number, currency)

    def handle_node(self, node: Node):
        """Obtain the parsed value of a node in the syntax tree.
        For named nodes in the grammar, try to handle them using a function
        from `.handlers`."""
        if node.is_named:
            handler = getattr(handlers, node.type)
            return handler(self, node)
        return node

    def get(self, node: Node, field: str) -> Optional[Any]:
        """Get the named node field."""
        child = node.child_by_field_name(field)
        if child is None:
            return None
        return self.handle_node(child)

def _recursive_parse(
    nodes: List[Node],
    state: ParserState,
    filename: Optional[str],
    seen_files: Set[str],
) -> Entries:
    """Parse the given file recursively.
    When an include directive is found, we recurse down. So the files are
    traversed in the order of a depth-first-search.
    Args:
        nodes: A list of top-level syntax tree nodes.
        state: The current ParserState (with .contents and .filename set for
            this file).
        filename: The absolute path to the file (if it is None, we do not
            recurse).
        seen_files: The set of already parsed files.
    """
    entries: Entries = []
    for node in nodes:
        try:
            res = state.handle_node(node)
            if res is not None:
                entries.append(res)
        except handlers.SyntaxError:
            node_contents = state.contents[
                node.start_byte : node.end_byte
            ].decode()
            state.error(
                node, f"Syntax error:\n{node_contents}\n{node.sexp()}",
            )
        except handlers.IncludeFound as incl:
            if filename is None:
                state.error(
                    node, "Cannot resolve include when parsing a string."
                )
                continue

            included_expanded = sorted(
                Path(filename).parent.glob(incl.filename)
            )
            if not included_expanded:
                state.error(node, "Include glob did not match any files.")
                continue

            for included in included_expanded:
                included_name = str(included.resolve())
                if included_name in seen_files:
                    state.error(node, f"Duplicate included file: {filename}")
                    continue
                contents = included.read_bytes()
                seen_files.add(included_name)
                with log_time(f"parsing {included_name}", LOG):
                    tree = PARSER.parse(contents)
                # Update state for the included file and recurse.
                with state.set_current_file(contents, included_name):
                    included_entries = _recursive_parse(
                        tree.root_node.children,
                        state,
                        included_name,
                        seen_files,
                    )
                    entries.extend(included_entries)
    return entries
