from contextlib import contextmanager
import logging
import time

def setup_logging(debug:bool):
    level = logging.DEBUG if debug else logging.INFO
    logging.basicConfig(level=level, format="%(message)s")
    logging.getLogger()

@contextmanager
def log_time(msg: str, logger: logging.Logger):
    """Context manager to time execution for debugging."""
    start = time.time()
    yield start
    end = time.time()
    if logger is not None:
        logger.debug("{}: {}ms".format(msg, end - start))
