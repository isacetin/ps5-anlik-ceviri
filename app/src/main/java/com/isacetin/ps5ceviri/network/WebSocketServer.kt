package com.isacetin.ps5ceviri.network

import android.util.Log
import com.isacetin.ps5ceviri.overlay.OverlayState
import io.ktor.server.application.install
import io.ktor.server.cio.CIO
import io.ktor.server.cio.CIOApplicationEngine
import io.ktor.server.engine.EmbeddedServer
import io.ktor.server.engine.embeddedServer
import io.ktor.server.routing.routing
import io.ktor.server.websocket.DefaultWebSocketServerSession
import io.ktor.server.websocket.WebSockets
import io.ktor.server.websocket.pingPeriod
import io.ktor.server.websocket.timeout
import io.ktor.server.websocket.webSocket
import io.ktor.websocket.Frame
import io.ktor.websocket.readText
import java.util.concurrent.atomic.AtomicInteger
import kotlin.time.Duration.Companion.seconds

/**
 * 0.0.0.0:[port] üzerinde dinleyen WebSocket server. Mac ws://TV_IP:8765 ile bağlanır.
 * Birden fazla istemci bağlanabilir; son gelen geçerli mesaj geçerlidir.
 */
class WebSocketServer(private val port: Int) {

    private var server: EmbeddedServer<CIOApplicationEngine, CIOApplicationEngine.Configuration>? = null
    private val clients = AtomicInteger(0)
    private var stopped = false

    /** Bloklayıcıdır (port bağlanana kadar); ana thread'den çağırma. */
    @Synchronized
    fun start() {
        // stop() start()'tan önce çalıştıysa (servis hemen kapandıysa) port açılmaz.
        if (server != null || stopped) return
        try {
            server = embeddedServer(CIO, port = port, host = "0.0.0.0") {
                install(WebSockets) {
                    pingPeriod = 15.seconds
                    timeout = 30.seconds
                }
                routing {
                    webSocket("/") { handleClient() }
                }
            }.start(wait = false)
            OverlayState.updateStatus { it.copy(serverRunning = true) }
            Log.i(TAG, "WebSocket server 0.0.0.0:$port üzerinde dinliyor")
        } catch (e: Exception) {
            // Port kullanımda vb. — servis çalışmaya devam etsin, durum ekranda "Kapalı" görünsün.
            Log.e(TAG, "WebSocket server başlatılamadı", e)
            server = null
        }
    }

    @Synchronized
    fun stop() {
        stopped = true
        server?.stop(gracePeriodMillis = 200, timeoutMillis = 1_000)
        server = null
        clients.set(0)
        OverlayState.updateStatus { it.copy(serverRunning = false, clientCount = 0) }
    }

    private suspend fun DefaultWebSocketServerSession.handleClient() {
        val remote = call.request.local.remoteAddress
        OverlayState.updateStatus { it.copy(clientCount = clients.incrementAndGet()) }
        Log.i(TAG, "İstemci bağlandı: $remote")
        // Seq sayacı her bağlantıda sıfırdan başlar.
        val seqGate = SeqGate()
        try {
            for (frame in incoming) {
                if (frame !is Frame.Text) continue
                val raw = frame.readText()
                val message = parseTranslationMessage(raw)
                if (message == null) {
                    Log.w(TAG, "Geçersiz mesaj yok sayıldı: ${raw.take(200)}")
                    continue
                }
                if (!seqGate.accept(message.seq)) {
                    Log.d(TAG, "Eski seq yok sayıldı: ${message.seq}")
                    continue
                }
                OverlayState.applyRemote(message.blocks)
            }
        } finally {
            OverlayState.updateStatus { it.copy(clientCount = clients.decrementAndGet().coerceAtLeast(0)) }
            Log.i(TAG, "İstemci ayrıldı: $remote")
        }
    }

    private companion object {
        const val TAG = "WebSocketServer"
    }
}
