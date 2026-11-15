package dev.aevia.jetbrains

import com.intellij.execution.configurations.PathEnvironmentVariableUtil
import com.intellij.openapi.project.Project
import com.redhat.devtools.lsp4ij.LanguageServerFactory
import com.redhat.devtools.lsp4ij.server.ProcessStreamConnectionProvider
import com.redhat.devtools.lsp4ij.server.StreamConnectionProvider
import java.nio.file.Files
import java.nio.file.Path

class AeviaLanguageServerFactory : LanguageServerFactory {
    override fun createConnectionProvider(project: Project): StreamConnectionProvider =
        AeviaConnectionProvider(project)
}

/** `$AEVIA_LSP`, else `aevia-lsp`/`aevia` on PATH, else common Cargo/local/workspace paths. */
private class AeviaConnectionProvider(project: Project) : ProcessStreamConnectionProvider() {
    init {
        commands = resolveCommand(project)
        project.basePath?.let { workingDirectory = it }
    }

    private fun resolveCommand(project: Project): List<String> {
        val fromEnv = System.getenv("AEVIA_LSP")?.takeIf { it.isNotBlank() }
        if (fromEnv != null) return listOf(fromEnv, "--stdio")
        if (PathEnvironmentVariableUtil.findInPath("aevia-lsp") != null) return listOf("aevia-lsp", "--stdio")
        if (PathEnvironmentVariableUtil.findInPath("aevia") != null) return listOf("aevia", "lsp", "--stdio")

        val home = System.getProperty("user.home")
        val candidates = buildList {
            if (!home.isNullOrBlank()) {
                add(Path.of(home, ".cargo", "bin", "aevia-lsp"))
                add(Path.of(home, ".local", "bin", "aevia-lsp"))
            }
            project.basePath?.let { base ->
                add(Path.of(base, "target", "release", "aevia-lsp"))
                add(Path.of(base, "target", "debug", "aevia-lsp"))
            }
        }
        val executable = candidates.firstOrNull { Files.isExecutable(it) }
        if (executable != null) return listOf(executable.toString(), "--stdio")

        return listOf("aevia-lsp", "--stdio")
    }
}
