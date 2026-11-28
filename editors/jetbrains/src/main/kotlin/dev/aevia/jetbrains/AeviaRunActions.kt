package dev.aevia.jetbrains

import com.intellij.execution.ExecutionException
import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.execution.executors.DefaultRunExecutor
import com.intellij.execution.filters.TextConsoleBuilderFactory
import com.intellij.execution.process.ProcessHandlerFactory
import com.intellij.execution.process.ProcessTerminatedListener
import com.intellij.execution.ui.RunContentDescriptor
import com.intellij.execution.ui.RunContentManager
import com.intellij.notification.NotificationGroupManager
import com.intellij.notification.NotificationType
import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.fileEditor.FileDocumentManager
import java.nio.file.Path

abstract class AeviaBaseRunAction(private val subcommand: String) : AnAction() {
    override fun getActionUpdateThread(): ActionUpdateThread = ActionUpdateThread.BGT

    override fun update(e: AnActionEvent) {
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE)
        e.presentation.isEnabledAndVisible =
            e.project != null && file != null && file.isInLocalFileSystem && !file.isDirectory && file.extension == "aev"
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE) ?: return
        if (!file.isInLocalFileSystem || file.extension != "aev") return

        FileDocumentManager.getInstance().saveAllDocuments()

        val executable = findAeviaTool("aevia") ?: "aevia"
        val workingDir = aeviaWorkingDirectory(Path.of(file.path)).toFile()
        val cmd = GeneralCommandLine(executable, subcommand, file.path)
            .withWorkDirectory(workingDir)
            .withParentEnvironmentType(GeneralCommandLine.ParentEnvironmentType.CONSOLE)

        try {
            val processHandler = ProcessHandlerFactory.getInstance().createColoredProcessHandler(cmd)
            ProcessTerminatedListener.attach(processHandler)
            val console = TextConsoleBuilderFactory.getInstance().createBuilder(project).console
            console.attachToProcess(processHandler)

            val title = "Aevia $subcommand: ${file.name}"
            val descriptor = RunContentDescriptor(console, processHandler, console.component, title)
            RunContentManager.getInstance(project)
                .showRunContent(DefaultRunExecutor.getRunExecutorInstance(), descriptor)
            processHandler.startNotify()
        } catch (ex: ExecutionException) {
            NotificationGroupManager.getInstance()
                .getNotificationGroup("Aevia")
                ?.createNotification(
                    "Failed to run `aevia $subcommand`: ${ex.message ?: "executable not found"}",
                    NotificationType.ERROR,
                )
                ?.notify(project)
        }
    }
}

class AeviaRunFileAction : AeviaBaseRunAction("run")
class AeviaTestFileAction : AeviaBaseRunAction("test")
