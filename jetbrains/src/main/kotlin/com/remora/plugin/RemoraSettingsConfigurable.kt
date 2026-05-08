package com.remora.plugin

import com.intellij.openapi.options.Configurable
import javax.swing.*

class RemoraSettingsConfigurable : Configurable {
    private var panel: JPanel? = null
    private var urlField: JTextField? = null
    private var tokenField: JPasswordField? = null
    private var nameField: JTextField? = null

    override fun getDisplayName(): String = "Remora"

    override fun createComponent(): JComponent {
        val settings = RemoraSettings.getInstance()
        val state = settings.state
        panel = JPanel().apply { layout = BoxLayout(this, BoxLayout.Y_AXIS) }
        urlField = JTextField(state.serverUrl, 30)
        tokenField = JPasswordField(settings.getToken(), 30)
        nameField = JTextField(state.displayName, 30)

        panel!!.add(JLabel("Server URL:"))
        panel!!.add(urlField)
        panel!!.add(JLabel("Token:"))
        panel!!.add(tokenField)
        panel!!.add(JLabel("Display Name:"))
        panel!!.add(nameField)
        return panel!!
    }

    override fun isModified(): Boolean {
        val settings = RemoraSettings.getInstance()
        val s = settings.state
        return urlField?.text != s.serverUrl ||
               String(tokenField?.password ?: charArrayOf()) != settings.getToken() ||
               nameField?.text != s.displayName
    }

    override fun apply() {
        val settings = RemoraSettings.getInstance()
        settings.loadState(RemoraSettings.State(
            serverUrl = urlField?.text ?: "",
            displayName = nameField?.text ?: ""
        ))
        settings.setToken(String(tokenField?.password ?: charArrayOf()))
    }

    override fun reset() {
        val settings = RemoraSettings.getInstance()
        val s = settings.state
        urlField?.text = s.serverUrl
        tokenField?.text = settings.getToken()
        nameField?.text = s.displayName
    }
}
