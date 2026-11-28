package dev.aevia.jetbrains

import com.intellij.openapi.application.PathManager
import org.jetbrains.plugins.textmate.api.TextMateBundleProvider
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.StandardCopyOption

/** Contributes the Aevia TextMate grammar (shared with the VS Code extension). */
class AeviaTextMateBundleProvider : TextMateBundleProvider {
    override fun getBundles(): List<TextMateBundleProvider.PluginBundle> {
        val target: Path = PathManager.getSystemDir().resolve("aevia-textmate")
        for (name in FILES) {
            val stream = javaClass.getResourceAsStream("/textmate/aevia/$name") ?: return emptyList()
            val out = target.resolve(name)
            Files.createDirectories(out.parent)
            stream.use { Files.copy(it, out, StandardCopyOption.REPLACE_EXISTING) }
        }
        return listOf(TextMateBundleProvider.PluginBundle("Aevia", target))
    }

    private companion object {
        val FILES = listOf("package.json", "language-configuration.json", "syntaxes/aevia.tmLanguage.json")
    }
}
