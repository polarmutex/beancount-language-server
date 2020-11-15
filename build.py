from distutils.command.build_ext import build_ext


ext_modules=[
    Extension(
        "beancount_language_server.tree_sitter_beancount",
        ["vendor/tree-sitter-beancount/src/parser.c"],
        include_dirs=["vendor/tree-sitter-beancount/src"],
        extra_compile_args=(),
    )
]


class BuildFailed(Exception):
    pass


class ExtBuilder(build_ext):

    def get_ext_filename(self, fullname):
        # Use simple file ending, since this extension doesn't depend on
        # Python.
        return os.path.join(*fullname.split(".")) + EXTENSION_SUFFIXES[-1]

    def get_export_symbols(self, ext):
        # On Windows, the existence of the PyINIT_x function is checked
        # otherwise. Since we're not building a Python extension but just a
        # library, that would fail.
        return None

    def run(self):
        try:
            build_ext.run(self)
        except (DistutilsPlatformError, FileNotFoundError):
            raise BuildFailed('File not found. Could not compile C extension.')

    def build_extension(self, ext):
        try:
            build_ext.build_extension(self, ext)
        except (CCompilerError, DistutilsExecError, DistutilsPlatformError, ValueError):
            raise BuildFailed('Could not compile C extension.')


def build(setup_kwargs):
    """
    This function is mandatory in order to build the extensions.
    """
    setup_kwargs.update(
        {"ext_modules": ext_modules, "cmdclass": {"build_ext": ExtBuilder}}
    )
