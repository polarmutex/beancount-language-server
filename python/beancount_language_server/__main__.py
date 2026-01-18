import sys

from .wrapper import main


def _main() -> int:
    args = list(sys.argv)
    args[0] = "python -m beancount_language_server"
    return main(args)


raise SystemExit(_main())
