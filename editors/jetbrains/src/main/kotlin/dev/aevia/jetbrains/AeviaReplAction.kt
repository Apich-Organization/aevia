package dev.aevia.jetbrains

import com.intellij.notification.NotificationGroupManager
import com.intellij.notification.NotificationType
import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import org.jetbrains.plugins.terminal.TerminalToolWindowManager
import java.nio.file.Path

class AeviaReplAction : AnAction() {
    override fun getActionUpdateThread(): ActionUpdateThread = ActionUpdateThread.BGT

    override fun update(event: AnActionEvent) {
        event.presentation.isEnabledAndVisible = event.project != null
    }

    override fun actionPerformed(event: AnActionEvent) {
        val project = event.project ?: return
        val cli = findAeviaTool("aevia")
        if (cli == null) {
            NotificationGroupManager.getInstance().getNotificationGroup("Aevia")
                .createNotification("Install aevia or configure AEVIA_CLI to start the REPL.", NotificationType.ERROR)
                .notify(project)
            return
        }
        val file = event.getData(CommonDataKeys.VIRTUAL_FILE)?.takeIf { it.isInLocalFileSystem }
        val cwd = if (file?.extension == "aev") aeviaWorkingDirectory(Path.of(file.path)).toString()
            else project.basePath
        TerminalToolWindowManager.getInstance(project)
            .createNewSession(cwd, "Aevia REPL", listOf(cli, "repl"), true, false)
    }
}
