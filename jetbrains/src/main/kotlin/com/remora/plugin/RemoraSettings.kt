package com.remora.plugin

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.components.*

@State(name = "RemoraSettings", storages = [Storage("remora.xml")])
@Service(Service.Level.APP)
class RemoraSettings : PersistentStateComponent<RemoraSettings.State> {
    data class State(
        var serverUrl: String = "",
        var token: String = "",
        var displayName: String = ""
    )

    private var myState = State()

    override fun getState(): State = myState
    override fun loadState(state: State) { myState = state }

    companion object {
        fun getInstance(): RemoraSettings =
            ApplicationManager.getApplication().getService(RemoraSettings::class.java)
    }
}
