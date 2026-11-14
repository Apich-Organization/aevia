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
        // otherwise against IntelliJ IDEA Community. The plugin itself works in
        // every JetBrains IDE from 2024.3 on: highlighting comes from the bundled
        // TextMate plugin, language-server features from LSP4IJ.
        val local = providers.environmentVariable("AEVIA_IDE_PATH").orNull
        if (local != null) local(local) else intellijIdeaCommunity("2025.2")
        bundledPlugin("org.jetbrains.plugins.textmate")
        plugin("com.redhat.devtools.lsp4ij", "0.19.1")
    }
}

kotlin { jvmToolchain(21) }

intellijPlatform {
    pluginConfiguration {
        name = "Aevia"
        ideaVersion {
            sinceBuild = "243"
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
