package dev.aevia.jetbrains

import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.execution.configurations.RunConfiguration
import com.intellij.execution.configurations.RunConfigurationOptions
import com.intellij.execution.runners.ExecutionEnvironment
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.redhat.devtools.lsp4ij.dap.DebugMode
import com.redhat.devtools.lsp4ij.dap.LaunchConfiguration
import com.redhat.devtools.lsp4ij.dap.configurations.DAPRunConfiguration
import com.redhat.devtools.lsp4ij.dap.configurations.DAPRunConfigurationOptions
import com.redhat.devtools.lsp4ij.dap.descriptors.DebugAdapterDescriptor
import com.redhat.devtools.lsp4ij.dap.descriptors.DebugAdapterDescriptorFactory
import com.redhat.devtools.lsp4ij.dap.descriptors.DefaultDebugAdapterDescriptor
import java.nio.file.Path

class AeviaDebugAdapterFactory : DebugAdapterDescriptorFactory() {
    private val launch = LaunchConfiguration(
        "aevia-launch",
        "Debug Aevia File",
        """{"type":"aevia","request":"launch","program":"${'$'}{file}","args":[]}""",
        DebugMode.LAUNCH,
    )

    override fun getLaunchConfigurations(): List<LaunchConfiguration> = listOf(launch)

    override fun isDebuggableFile(file: VirtualFile, project: Project): Boolean =
        file.isInLocalFileSystem && !file.isDirectory && file.extension == "aev"

    override fun prepareConfiguration(configuration: RunConfiguration, file: VirtualFile, project: Project): Boolean {
        if (!isDebuggableFile(file, project) || !super.prepareConfiguration(configuration, file, project)) return false
        if (configuration is DAPRunConfiguration) {
            configuration.launchConfigurationId = launch.id
            configuration.launchConfiguration = launch.content
            configuration.workingDirectory = aeviaWorkingDirectory(Path.of(file.path)).toString()
        }
        return true
    }

    override fun createDebugAdapterDescriptor(
        options: RunConfigurationOptions,
        environment: ExecutionEnvironment,
    ): DebugAdapterDescriptor =
        object : DefaultDebugAdapterDescriptor(options, environment, serverDefinition) {
            override fun createStartServerCommandLine(options: RunConfigurationOptions): GeneralCommandLine? {
                if (options is DAPRunConfigurationOptions && !options.command.isNullOrBlank()) {
                    return super.createStartServerCommandLine(options)
                }
                val env = envData
                val parent = if (env?.isPassParentEnvs == false) GeneralCommandLine.ParentEnvironmentType.NONE
                    else GeneralCommandLine.ParentEnvironmentType.CONSOLE
                val command = GeneralCommandLine(findAeviaTool("aevia") ?: "aevia", "debug", "--stdio")
                    .withParentEnvironmentType(parent)
                    .withWorkDirectory(environment.project.basePath)
                env?.let { command.withEnvironment(it.envs) }
                return command
            }

            override fun isDebuggableFile(file: VirtualFile, project: Project): Boolean =
                this@AeviaDebugAdapterFactory.isDebuggableFile(file, project)
        }
}
