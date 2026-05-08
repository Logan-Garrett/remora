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
        val settings = RemoraSettings.getInstance().state
        panel = JPanel().apply { layout = BoxLayout(this, BoxLayout.Y_AXIS) }
        urlField = JTextField(settings.serverUrl, 30)
        tokenField = JPasswordField(settings.token, 30)
        nameField = JTextField(settings.displayName, 30)

        panel!!.add(JLabel("Server URL:"))
        panel!!.add(urlField)
        panel!!.add(JLabel("Token:"))
        panel!!.add(tokenField)
        panel!!.add(JLabel("Display Name:"))
        panel!!.add(nameField)
        return panel!!
    }

    override fun isModified(): Boolean {
        val s = RemoraSettings.getInstance().state
        return urlField?.text != s.serverUrl ||
               String(tokenField?.password ?: charArrayOf()) != s.token ||
               nameField?.text != s.displayName
    }

    override fun apply() {
        val s = RemoraSettings.getInstance()
        s.loadState(RemoraSettings.State(
            serverUrl = urlField?.text ?: "",
            token = String(tokenField?.password ?: charArrayOf()),
            displayName = nameField?.text ?: ""
        ))
    }

    override fun reset() {
        val s = RemoraSettings.getInstance().state
        urlField?.text = s.serverUrl
        tokenField?.text = s.token
        nameField?.text = s.displayName
    }
}
