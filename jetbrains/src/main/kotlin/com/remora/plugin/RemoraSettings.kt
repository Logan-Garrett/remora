package com.remora.plugin

import com.intellij.credentialStore.CredentialAttributes
import com.intellij.credentialStore.generateServiceName
import com.intellij.ide.passwordSafe.PasswordSafe
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.components.*

@State(name = "RemoraSettings", storages = [Storage("remora.xml")])
@Service(Service.Level.APP)
class RemoraSettings : PersistentStateComponent<RemoraSettings.State> {
    data class State(
        var serverUrl: String = "",
        // Token is stored in PasswordSafe, NOT in this XML file
        var displayName: String = ""
    )

    private var myState = State()

    override fun getState(): State = myState
    override fun loadState(state: State) { myState = state }

    /** Store the auth token securely in the OS keychain via PasswordSafe. */
    fun setToken(token: String) {
        val attrs = CredentialAttributes(generateServiceName("Remora", "token"))
        PasswordSafe.instance.setPassword(attrs, token)
    }

    /** Retrieve the auth token from the OS keychain. */
    fun getToken(): String {
        val attrs = CredentialAttributes(generateServiceName("Remora", "token"))
        return PasswordSafe.instance.getPassword(attrs) ?: ""
    }

    companion object {
        fun getInstance(): RemoraSettings =
            ApplicationManager.getApplication().getService(RemoraSettings::class.java)
    }
}
