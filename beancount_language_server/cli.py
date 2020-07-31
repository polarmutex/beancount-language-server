import click
import logging
import logging.config

from beancount_language_server.server import SERVER


@click.command()
@click.option("--debug", is_flag=True)
@click.option("--tcp", is_flag=True)
@click.option("--host", default="127.0.0.1")
@click.option("--port", type=int, default=2087)
@click.option("--log-file", type=click.Path())
def cli(debug, tcp, host, port, log_file) -> None:

    root_logger = logging.root

    formatter = logging.Formatter("%(asctime)s UTC - %(levelname)s - %(name)s - %(message)s")
    if log_file:
        log_handler = logging.handlers.RotatingFileHandler(
            log_file, mode='a', maxBytes=50*1024*1024,
            backupCount=10, encoding=None, delay=0
        )
    else:
        log_handler = logging.StreamHandler()

    log_handler.setFormatter(formatter)
    root_logger.addHandler(log_handler)

    if debug:
        level = logging.DEBUG
    else:
        level = logging.INFO
    root_logger.setLevel(level)

    """
    Beancount Language Server
    """
    if tcp:
        SERVER.start_tcp(host, port)
    else:
        SERVER.start_io()


if __name__ == '__main__':
    cli()
