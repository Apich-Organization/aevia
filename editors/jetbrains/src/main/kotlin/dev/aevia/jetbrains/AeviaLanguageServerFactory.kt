package dev.aevia.jetbrains

import com.intellij.openapi.project.Project
import com.redhat.devtools.lsp4ij.LanguageServerFactory
import com.redhat.devtools.lsp4ij.server.ProcessStreamConnectionProvider
import com.redhat.devtools.lsp4ij.server.StreamConnectionProvider

class AeviaLanguageServerFactory : LanguageServerFactory {
    override fun createConnectionProvider(project: Project): StreamConnectionProvider =
        AeviaConnectionProvider(project)
}

private class AeviaConnectionProvider(project: Project) : ProcessStreamConnectionProvider() {
    init {
        val server = findAeviaTool("aevia-lsp")
        val cli = if (server == null) findAeviaTool("aevia") else null
        commands = when {
            server != null -> listOf(server, "--stdio")
            cli != null -> listOf(cli, "lsp", "--stdio")
            else -> listOf("aevia-lsp", "--stdio")
        }
        project.basePath?.let { workingDirectory = it }
    }
}
