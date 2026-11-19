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

// One TextMate grammar for VS Code and JetBrains.
tasks.processResources {
    from("../vscode/syntaxes") { into("textmate/aevia/syntaxes") }
    from("../vscode/language-configuration.json") { into("textmate/aevia") }
}
