package dev.aevia.jetbrains

import com.intellij.execution.configurations.PathEnvironmentVariableUtil
import com.intellij.openapi.project.Project
import com.redhat.devtools.lsp4ij.LanguageServerFactory
import com.redhat.devtools.lsp4ij.server.ProcessStreamConnectionProvider
import com.redhat.devtools.lsp4ij.server.StreamConnectionProvider

class AeviaLanguageServerFactory : LanguageServerFactory {
    override fun createConnectionProvider(project: Project): StreamConnectionProvider =
        AeviaConnectionProvider(project)
}

/** `$AEVIA_LSP`, else `aevia-lsp` on PATH, else `aevia lsp`. */
private class AeviaConnectionProvider(project: Project) : ProcessStreamConnectionProvider() {
    init {
        val fromEnv = System.getenv("AEVIA_LSP")?.takeIf { it.isNotBlank() }
        val command = when {
            fromEnv != null -> listOf(fromEnv, "--stdio")
            PathEnvironmentVariableUtil.findInPath("aevia-lsp") != null -> listOf("aevia-lsp", "--stdio")
            PathEnvironmentVariableUtil.findInPath("aevia") != null -> listOf("aevia", "lsp", "--stdio")
            else -> listOf("aevia-lsp", "--stdio")
        }
        commands = command
        project.basePath?.let { workingDirectory = it }
    }
}
