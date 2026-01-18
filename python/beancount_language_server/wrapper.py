from __future__ import annotations

import sys
from typing import Sequence

from ._beancount_lsp import main as _main


def main(argv: Sequence[str] | None = None) -> int:
    if argv is None:
        argv = sys.argv
    return _main(list(argv))
