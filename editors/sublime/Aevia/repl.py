"""Launch the Aevia REPL in the optional Terminus terminal."""

import os

import sublime
import sublime_plugin


class AeviaOpenReplCommand(sublime_plugin.WindowCommand):
    def run(self):
        try:
            sublime.load_resource("Packages/Terminus/Terminus.sublime-settings")
        except IOError:
            sublime.status_message("Install Terminus or run aevia repl in a terminal.")
            return

        view = self.window.active_view()
        file_name = view.file_name() if view else None
        folders = self.window.folders()
        cwd = os.path.dirname(file_name) if file_name else (folders[0] if folders else None)
        for directory in parent_directories(cwd):
            if os.path.isfile(os.path.join(directory, "aevia.toml")):
                cwd = directory
                break
        args = {"cmd": ["aevia", "repl"], "title": "Aevia REPL"}
        if cwd:
            args["cwd"] = cwd
        self.window.run_command("terminus_open", args)


def parent_directories(path):
    while path:
        yield path
        parent = os.path.dirname(path)
        if parent == path:
            return
        path = parent
