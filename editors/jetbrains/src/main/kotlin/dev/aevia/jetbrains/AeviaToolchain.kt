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
    val home = System.getProperty("user.home")?.takeIf { it.isNotBlank() }
    val managed = System.getenv("AEVIA_HOME")?.takeIf { it.isNotBlank() }?.let { Path.of(it) }
        ?: if (SystemInfo.isWindows) {
            System.getenv("LOCALAPPDATA")?.takeIf { it.isNotBlank() }?.let { Path.of(it, "aevia") }
        } else {
            home?.let { Path.of(it, ".aevia") }
        }
    return listOfNotNull(
        managed?.resolve("bin"),
        home?.let { Path.of(it, ".cargo", "bin") },
        home?.let { Path.of(it, ".local", "bin") },
    ).asSequence()
        .map { it.resolve(executable) }
        .firstOrNull { Files.isRegularFile(it) && Files.isExecutable(it) }
        ?.toString()
}

internal fun aeviaWorkingDirectory(file: Path): Path {
    val directory = file.toAbsolutePath().parent ?: file
    return generateSequence(directory) { it.parent }
        .firstOrNull { Files.isRegularFile(it.resolve("aevia.toml")) } ?: directory
}
