import logging

from beancount.loader import _load

from beancount_language_server.util import log_time


class Parser(object):
    def __init__(self, use_tree_sitter=False):
        self.root_file = None
        self.use_tree_sitter =  use_tree_sitter
        self.is_encrypted = False
        self.logger = logging.getLogger()

    def set_root_file(self, root_file):
        self.root_file = root_file

    def open(self):

        bc_entries, bc_errors, bc_options = self._beancount_load_file()

        if self.is_encrypted or not self.use_tree_sitter:
            return bc_entries, bc_errors, bc_options

        ts_entries, ts_errors, ts_options = self._tree_sitter_load_file()

        for left, right in zip(bc_entries, ts_entries):
            if left != right:
                logger.info(f"MisMatch:\n{left}\n{right}")
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
                entries, parse_errors, options_map = self._tree_sitter_parse_files()

            with log_time("Tree-Sitter Post Process", self.logger):
                self._tree_sitter_parse_files()

    def _tree_sitter_parse_files(self):
        entries, parse_errors, options_map = parse_file(self.root_file)
        entries.sort(key=entry_sortkey)
        return entries, parse_errors, options_map

    def _tree_sitter_post_process(self, entries, parse_errors, options_map):
        entries, errors = run_transformations(entries, parse_errors, options_map, None)

        valid_errors =  validation.validate(entries, options_map, None, None)
        errors.extend(valid_errors)

        options_map["input_hash"] = compute_input_hash(options_map["include"])

        return entries, errors, options_map
