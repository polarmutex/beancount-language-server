import argparse
import click
import logging
import logging.config
import sys

from .server import SERVER

logging.basicConfig(filename="bls.log", level=logging.DEBUG, filemode="w")

@click.command()
def cli() -> None:
    """
    Beancount Language Server
    """
    SERVER.start_io()

if __name__ == '__main__':
    cli()
