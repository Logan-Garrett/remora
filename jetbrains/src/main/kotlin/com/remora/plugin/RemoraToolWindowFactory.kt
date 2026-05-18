package com.remora.plugin

import com.intellij.openapi.project.Project
import com.intellij.openapi.wm.ToolWindow
import com.intellij.openapi.wm.ToolWindowFactory
import com.intellij.ui.content.ContentFactory
import javax.swing.*
import java.awt.*

class RemoraToolWindowFactory : ToolWindowFactory {
    override fun createToolWindowContent(project: Project, toolWindow: ToolWindow) {
        val panel = RemoraPanel(project)
        val content = ContentFactory.getInstance().createContent(panel, "Chat", false)
        toolWindow.contentManager.addContent(content)
    }
}

class RemoraPanel(private val project: Project) : JPanel(BorderLayout()) {
    private val chatArea = JTextArea().apply {
        isEditable = false
        lineWrap = true
        wrapStyleWord = true
        font = Font(Font.MONOSPACED, Font.PLAIN, 12)
        background = Color(0x1a, 0x1b, 0x26)
        foreground = Color(0xcd, 0xd6, 0xf4)
    }
    private val inputField = JTextField()
    private val connectBtn = JButton("Connect")
    private var connection: RemoraConnection? = null

    init {
        add(JScrollPane(chatArea), BorderLayout.CENTER)

        val bottomPanel = JPanel(BorderLayout())
        bottomPanel.add(inputField, BorderLayout.CENTER)
        bottomPanel.add(connectBtn, BorderLayout.EAST)
        add(bottomPanel, BorderLayout.SOUTH)

        connectBtn.addActionListener { toggleConnection() }
        inputField.addActionListener { sendMessage() }
    }

    private fun toggleConnection() {
        if (connection != null) {
            connection?.disconnect()
            connection = null
            connectBtn.text = "Connect"
            appendChat("[System] Disconnected\n")
            return
        }

        val settings = RemoraSettings.getInstance()
        val state = settings.state
        val token = settings.getToken()
        if (state.serverUrl.isEmpty() || token.isEmpty()) {
            appendChat("[Error] Configure server URL and token in Settings > Tools > Remora\n")
            return
        }

        // Simple session picker -- in a real impl this would show a dialog
        val sessionId = JOptionPane.showInputDialog(this, "Session ID:")
        if (sessionId.isNullOrBlank()) return

        val name = state.displayName.ifEmpty { "jetbrains-user" }
        connection = RemoraConnection(
            url = state.serverUrl,
            token = token,
            sessionId = sessionId,
            name = name,
            onMessage = { msg -> SwingUtilities.invokeLater { handleMessage(msg) } },
            onStatus = { status -> SwingUtilities.invokeLater {
                appendChat("[System] $status\n")
                connectBtn.text = if (status == "connected") "Disconnect" else "Connect"
            }}
        )
        connection?.connect(kotlinx.coroutines.CoroutineScope(kotlinx.coroutines.Dispatchers.Default))
    }

    private fun handleMessage(msg: com.google.gson.JsonObject) {
        when (msg.get("type")?.asString) {
            "event" -> {
                val data = msg.getAsJsonObject("data")
                val kind = data?.get("kind")?.asString ?: ""
                val author = data?.get("author")?.asString ?: ""
                val payload = data?.getAsJsonObject("payload")
                val text = payload?.get("text")?.asString ?: ""
                when (kind) {
                    "chat" -> appendChat("[$author] $text\n")
                    "system" -> appendChat("[system] $text\n")
                    "claude_response" -> appendChat("[Claude] $text\n")
                    "tool_call" -> {
                        val tool = payload?.get("tool")?.asString ?: ""
                        appendChat("[tool: $tool]\n")
                    }
                    "tool_result" -> {
                        val output = payload?.get("output")?.asString ?: ""
                        val lines = output.lines().take(5).joinToString("\n")
                        appendChat("[result] $lines\n")
                    }
                    else -> appendChat("[$kind] $text\n")
                }
            }
            "stream_delta" -> {
                val delta = msg.get("delta")?.asString ?: ""
                chatArea.append(delta)
            }
            "stream_start" -> appendChat("[Claude is generating...]\n")
            "stream_end" -> appendChat("\n")
            "error" -> appendChat("[Error] ${msg.get("message")?.asString}\n")
        }
    }

    private fun sendMessage() {
        val text = inputField.text.trim()
        if (text.isEmpty()) return
        inputField.text = ""

        val settings = RemoraSettings.getInstance().state
        val name = settings.displayName.ifEmpty { "jetbrains-user" }

        val msg = parseCommand(text, name)
        connection?.send(msg)
    }

    private fun parseCommand(input: String, author: String): Map<String, Any> {
        if (!input.startsWith("/")) return mapOf("type" to "chat", "author" to author, "text" to input)
        val parts = input.split(" ", limit = 2)
        val cmd = parts[0].lowercase()
        val arg = parts.getOrNull(1)?.trim() ?: ""
        return when (cmd) {
            "/run" -> mapOf("type" to "run", "author" to author)
            "/run-all", "/runall" -> mapOf("type" to "run_all", "author" to author)
            "/who" -> mapOf("type" to "who", "author" to author)
            "/help", "/?" -> mapOf("type" to "help", "author" to author)
            "/clear" -> mapOf("type" to "clear", "author" to author)
            "/diff" -> mapOf("type" to "diff", "author" to author)
            "/session", "/info" -> mapOf("type" to "session_info", "author" to author)
            "/add" -> mapOf("type" to "add", "author" to author, "path" to arg)
            "/fetch" -> mapOf("type" to "fetch", "author" to author, "url" to arg)
            "/trust" -> mapOf("type" to "trust", "author" to author, "target" to arg)
            "/untrust" -> mapOf("type" to "untrust", "author" to author, "target" to arg)
            "/kick" -> mapOf("type" to "kick", "author" to author, "target" to arg)
            else -> mapOf("type" to "chat", "author" to author, "text" to input)
        }
    }

    private fun appendChat(text: String) {
        chatArea.append(text)
        chatArea.caretPosition = chatArea.document.length
    }
}
