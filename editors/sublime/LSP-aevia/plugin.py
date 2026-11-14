"""LSP-aevia: wires the `aevia-lsp` binary into Sublime's LSP package.

The server is not downloaded: install the Aevia toolchain so that
`aevia-lsp` (or the `aevia` CLI) is on PATH, or set "command" in
Preferences > Package Settings > LSP > Servers > LSP-aevia.
"""
import sublime
from LSP.plugin import AbstractPlugin, register_plugin, unregister_plugin


class AeviaLsp(AbstractPlugin):
    @classmethod
    def name(cls) -> str:
        return 'aevia'

    @classmethod
    def needs_update_or_installation(cls) -> bool:
        return False


def plugin_loaded() -> None:
    register_plugin(AeviaLsp)


def plugin_unloaded() -> None:
    unregister_plugin(AeviaLsp)
