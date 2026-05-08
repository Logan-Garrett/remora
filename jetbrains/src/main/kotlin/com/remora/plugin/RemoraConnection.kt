package com.remora.plugin

import com.google.gson.Gson
import com.google.gson.JsonObject
import io.ktor.client.*
import io.ktor.client.engine.cio.*
import io.ktor.client.plugins.websocket.*
import io.ktor.websocket.*
import kotlinx.coroutines.*

class RemoraConnection(
    private val url: String,
    private val token: String,
    private val sessionId: String,
    private val name: String,
    private val onMessage: (JsonObject) -> Unit,
    private val onStatus: (String) -> Unit
) {
    private val gson = Gson()
    private var job: Job? = null
    private var session: DefaultClientWebSocketSession? = null
    private val client = HttpClient(CIO) { install(WebSockets) }

    fun connect(scope: CoroutineScope) {
        job = scope.launch(Dispatchers.IO) {
            val wsUrl = url.replace(Regex("^http"), "ws") +
                "/sessions/$sessionId?token=${java.net.URLEncoder.encode(token, "UTF-8")}" +
                "&name=${java.net.URLEncoder.encode(name, "UTF-8")}"

            try {
                client.webSocket(wsUrl) {
                    session = this
                    onStatus("connected")
                    for (frame in incoming) {
                        if (frame is Frame.Text) {
                            try {
                                val json = gson.fromJson(frame.readText(), JsonObject::class.java)
                                onMessage(json)
                            } catch (_: Exception) {}
                        }
                    }
                }
            } catch (_: Exception) {
                onStatus("disconnected")
            }
        }
    }

    fun send(msg: Any) {
        val json = gson.toJson(msg)
        session?.let { s ->
            CoroutineScope(Dispatchers.IO).launch {
                s.send(Frame.Text(json))
            }
        }
    }

    fun disconnect() {
        job?.cancel()
        session = null
    }
}
