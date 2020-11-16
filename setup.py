from os import path
from platform import system
from setuptools import Extension, find_packages, setup
from setuptools.command.build_ext import build_ext
from importlib.machinery import EXTENSION_SUFFIXES
import os

with open(path.join(path.dirname(__file__), "README.md")) as f:
    LONG_DESCRIPTION = f.read()

class BuildTreeSitter(build_ext):
    """Build a tree_sitter grammar."""

    def get_ext_filename(self, fullname):
        # Use simple file ending, since this extension doesn't depend on
        # Python.
        return os.path.join(*fullname.split(".")) + EXTENSION_SUFFIXES[-1]

    def get_export_symbols(self, ext):
        # On Windows, the existence of the PyINIT_x function is checked
        # otherwise. Since we're not building a Python extension but just a
        # library, that would fail.
        return None

setup(
    name="beancount_language_server",
    version="0.2.0",
    maintainer="Brian Ryall",
    maintainer_email="bryall@gmail.com",
    author="Brian Ryall",
    url="https://github.com/bryall/beancount-language-server",
    license="MIT",
    platforms=['any'],
    python_requires=">=3.7",
    description="LSP for beancount",
    long_description=LONG_DESCRIPTION,
    long_description_type="text/markdown",
    classifiers=[
        "License :: OSI Approved :: MIT License",
    ],
    packages=find_packages(),
    install_requires=[
        'Click',
        'beancount',
        'tree_sitter',
        'pygls'
    ],
    entry_points='''
        [console_scripts]
        beancount-language-server=beancount_language_server.cli:cli
    ''',
    cmdclass={"build_ext": BuildTreeSitter},
    ext_modules=[
        Extension(
            "beancount_language_server.tree_sitter_beancount",
            ["vendor/tree-sitter-beancount/src/parser.c"],
            include_dirs=["vendor/tree-sitter-beancount/src"],
            extra_compile_args=(),
        )
    ]
)
