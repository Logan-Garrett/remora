package com.remora.plugin

import com.google.gson.Gson
import com.google.gson.JsonObject
import com.google.gson.JsonSyntaxException
import com.intellij.openapi.diagnostic.Logger
import io.ktor.client.*
import io.ktor.client.engine.cio.*
import io.ktor.client.plugins.websocket.*
import io.ktor.websocket.*
import kotlinx.coroutines.*
import java.util.concurrent.atomic.AtomicReference
import kotlin.coroutines.cancellation.CancellationException

class RemoraConnection(
    private val url: String,
    private val token: String,
    private val sessionId: String,
    private val name: String,
    private val onMessage: (JsonObject) -> Unit,
    private val onStatus: (String) -> Unit
) {
    private val log = Logger.getInstance(RemoraConnection::class.java)
    private val gson = Gson()
    private var job: Job? = null
    private val sessionRef = AtomicReference<DefaultClientWebSocketSession?>(null)
    private val client = HttpClient(CIO) { install(WebSockets) }
    private val sendScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    fun connect(scope: CoroutineScope) {
        job = scope.launch(Dispatchers.IO) {
            val wsUrl = url.replace(Regex("^http"), "ws") +
                "/sessions/$sessionId?token=${java.net.URLEncoder.encode(token, "UTF-8")}" +
                "&name=${java.net.URLEncoder.encode(name, "UTF-8")}"

            try {
                client.webSocket(wsUrl) {
                    sessionRef.set(this)
                    onStatus("connected")
                    for (frame in incoming) {
                        if (frame is Frame.Text) {
                            try {
                                val json = gson.fromJson(frame.readText(), JsonObject::class.java)
                                onMessage(json)
                            } catch (e: JsonSyntaxException) {
                                log.warn("Remora: malformed server message", e)
                            }
                        }
                    }
                }
            } catch (e: CancellationException) {
                // Respect structured concurrency — rethrow cancellation
                throw e
            } catch (e: Exception) {
                log.info("Remora: connection closed: ${e.message}")
                onStatus("disconnected")
            } finally {
                sessionRef.set(null)
            }
        }
    }

    fun send(msg: Any) {
        val json = gson.toJson(msg)
        val s = sessionRef.get() ?: return
        sendScope.launch {
            try {
                s.send(Frame.Text(json))
            } catch (_: Exception) {
                // Connection may have closed between get() and send()
            }
        }
    }

    fun disconnect() {
        job?.cancel()
        sessionRef.set(null)
        sendScope.cancel()
        client.close()
    }
}
