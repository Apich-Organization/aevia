import groovy.json.JsonOutput
import groovy.json.JsonSlurper

plugins {
    id("org.jetbrains.kotlin.jvm") version "2.1.20"
    id("org.jetbrains.intellij.platform") version "2.10.2"
}

group = "dev.aevia"
version = "0.1.0"

repositories {
    mavenCentral()
    intellijPlatform { defaultRepositories() }
}

dependencies {
    intellijPlatform {
        // Build against an installed IDE when AEVIA_IDE_PATH is set (no download);
        // otherwise against IntelliJ IDEA Community. TextMate supplies
        // highlighting, and LSP4IJ supplies the language and debug clients.
        val local = providers.environmentVariable("AEVIA_IDE_PATH").orNull
        if (local != null) local(local) else intellijIdeaCommunity("2025.2")
        bundledPlugin("org.jetbrains.plugins.textmate")
        bundledPlugin("org.jetbrains.plugins.terminal")
        plugin("com.redhat.devtools.lsp4ij", "0.19.1")
    }
}

kotlin { jvmToolchain(21) }

intellijPlatform {
    pluginConfiguration {
        name = "Aevia"
        ideaVersion {
            sinceBuild = "252"
            untilBuild = provider { null }
        }
    }
    buildSearchableOptions = false
}

// Bundle the grammar and registration metadata from the VS Code extension.
val textMateManifest = tasks.register("generateTextMateManifest") {
    val source = layout.projectDirectory.file("../vscode/package.json")
    val output = layout.buildDirectory.file("generated/textmate/package.json")
    inputs.file(source)
    outputs.file(output)

    doLast {
        val extension = JsonSlurper().parse(source.asFile) as Map<*, *>
        val contributions = extension["contributes"] as Map<*, *>
        val registration = listOf("languages", "grammars").associateWith { key ->
            requireNotNull(contributions[key]) { "Missing TextMate contribution $key" }
        }
        val bundle = listOf("name", "displayName", "version").associateWith { key ->
            requireNotNull(extension[key]) { "Missing TextMate bundle metadata $key" }
        } + ("contributes" to registration)
        output.get().asFile.apply {
            parentFile.mkdirs()
            writeText(JsonOutput.prettyPrint(JsonOutput.toJson(bundle)) + "\n", Charsets.UTF_8)
        }
    }
}

tasks.processResources {
    from(textMateManifest) { into("textmate/aevia") }
    from("../vscode/syntaxes") { into("textmate/aevia/syntaxes") }
    from("../vscode/language-configuration.json") { into("textmate/aevia") }
}
