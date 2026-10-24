package com.isacetin.ps5ceviri.overlay

import com.isacetin.ps5ceviri.network.Block
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** Şimdilik yalnızca POSITION uygulanıyor; SUBTITLE ileride ekranın alt bölümünde gösterecek. */
enum class DisplayMode { POSITION, SUBTITLE }

data class ServiceStatus(
    val serviceRunning: Boolean = false,
    val overlayAttached: Boolean = false,
    val serverRunning: Boolean = false,
    val clientCount: Int = 0,
)

/** Son geçerli WebSocket mesajından bu kadar süre sonra WebSocket blokları temizlenir. */
const val REMOTE_TIMEOUT_MILLIS = 3_000L

/**
 * Activity, WebSocket server ve overlay arasındaki ortak state.
 * Demo blokları ve WebSocket blokları ayrı tutulur; overlay ikisinin birleşimini çizer.
 * Timeout yalnızca WebSocket bloklarına uygulanır; demo bloğu kaldırılana kadar kalır.
 */
class OverlayStore(
    private val scope: CoroutineScope,
    private val timeoutMillis: Long = REMOTE_TIMEOUT_MILLIS,
) {
    private var demoBlocks = emptyList<Block>()
    private var remoteBlocks = emptyList<Block>()
    private var timeoutJob: Job? = null

    // Süresi dolmuş ama kilidi bekleyen eski bir timer'ın yeni gelen blokları silmesini engeller.
    private var remoteGeneration = 0L

    private val _blocks = MutableStateFlow<List<Block>>(emptyList())
    val blocks: StateFlow<List<Block>> = _blocks.asStateFlow()

    private val _status = MutableStateFlow(ServiceStatus())
    val status: StateFlow<ServiceStatus> = _status.asStateFlow()

    fun showDemo() = change { demoBlocks = listOf(DEMO_BLOCK) }

    /** "Test yazısını kaldır": demo dahil tüm blokları temizler. */
    fun clearAll() = change {
        timeoutJob?.cancel()
        remoteGeneration++
        demoBlocks = emptyList()
        remoteBlocks = emptyList()
    }

    /**
     * Geçerli bir WebSocket mesajının blok listesi. Önceki listenin yerini alır ve liste aynı
     * olsa bile timeout'u sıfırlar (heartbeat). Boş liste WebSocket bloklarını hemen temizler.
     */
    fun applyRemote(blocks: List<Block>) = change {
        remoteBlocks = blocks
        timeoutJob?.cancel()
        val generation = ++remoteGeneration
        timeoutJob = if (blocks.isEmpty()) {
            null
        } else {
            scope.launch {
                delay(timeoutMillis)
                change { if (remoteGeneration == generation) remoteBlocks = emptyList() }
            }
        }
    }

    fun updateStatus(transform: (ServiceStatus) -> ServiceStatus) = _status.update(transform)

    @Synchronized
    private fun change(block: () -> Unit) {
        block()
        _blocks.value = demoBlocks + remoteBlocks
    }

    companion object {
        val DEMO_BLOCK = Block(
            id = "demo-1",
            x = 0.25f,
            y = 0.42f,
            w = 0.50f,
            h = 0.10f,
            lineH = 0.04f,
            text = "KAPI KİLİTLİ",
        )
    }
}

/** Uygulama genelindeki tek örnek. */
val OverlayState = OverlayStore(CoroutineScope(SupervisorJob() + Dispatchers.Default))
