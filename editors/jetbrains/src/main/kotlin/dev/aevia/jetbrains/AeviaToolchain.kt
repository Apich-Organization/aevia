package dev.aevia.jetbrains

import com.intellij.execution.configurations.PathEnvironmentVariableUtil
import com.intellij.openapi.util.SystemInfo
import java.nio.file.Files
import java.nio.file.Path

internal fun findAeviaTool(name: String): String? {
    val env = if (name == "aevia-lsp") "AEVIA_LSP" else "AEVIA_CLI"
    System.getenv(env)?.trim()?.takeIf { it.isNotEmpty() }?.let { return it }
    val executable = if (SystemInfo.isWindows) "$name.exe" else name
    PathEnvironmentVariableUtil.findInPath(executable)?.let { return it.absolutePath }
    val home = System.getProperty("user.home")?.takeIf { it.isNotBlank() } ?: return null
    return listOf(Path.of(home, ".cargo", "bin", executable), Path.of(home, ".local", "bin", executable))
        .firstOrNull { Files.isRegularFile(it) && Files.isExecutable(it) }
        ?.toString()
}

internal fun aeviaWorkingDirectory(file: Path): Path {
    val directory = file.toAbsolutePath().parent ?: file
    return generateSequence(directory) { it.parent }
        .firstOrNull { Files.isRegularFile(it.resolve("aevia.toml")) } ?: directory
}
