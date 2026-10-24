package com.isacetin.ps5ceviri.ui

import android.app.Application
import android.net.ConnectivityManager
import android.provider.Settings
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.isacetin.ps5ceviri.overlay.OverlayService
import com.isacetin.ps5ceviri.overlay.OverlayState
import com.isacetin.ps5ceviri.overlay.ServiceStatus
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import java.net.Inet4Address
import java.net.InetAddress
import java.net.NetworkInterface

data class MainUiState(
    val ipAddress: String? = null,
    val port: Int = OverlayService.WS_PORT,
    val overlayPermission: Boolean = false,
    val status: ServiceStatus = ServiceStatus(),
    val message: String? = null,
)

class MainViewModel(application: Application) : AndroidViewModel(application) {

    private val local = MutableStateFlow(MainUiState())

    val uiState: StateFlow<MainUiState> = combine(local, OverlayState.status) { state, status ->
        state.copy(status = status)
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), MainUiState())

    /** Ekrana her dönüldüğünde (ör. izin ayarından sonra) izin ve IP yeniden okunur. */
    fun refresh() {
        val app = getApplication<Application>()
        val permission = Settings.canDrawOverlays(app)
        local.update {
            it.copy(
                ipAddress = localIpAddress(app),
                overlayPermission = permission,
                message = if (permission) null else it.message,
            )
        }
        // İzin servis çalışırken verildiyse overlay'i eklemesi için servisi dürt.
        val status = OverlayState.status.value
        if (permission && status.serviceRunning && !status.overlayAttached) {
            OverlayService.start(app)
        }
    }

    fun toggleOverlay() {
        val app = getApplication<Application>()
        if (OverlayState.status.value.serviceRunning) OverlayService.stop(app) else OverlayService.start(app)
    }

    fun showTestText() {
        OverlayState.showDemo()
        if (!OverlayState.status.value.serviceRunning) OverlayService.start(getApplication())
    }

    fun clearTestText() = OverlayState.clearAll()

    fun onPermissionScreenUnavailable() {
        local.update { it.copy(message = "Bu TV'de izin ekranı açılamadı. Aşağıdaki adb komutunu kullan.") }
    }

    private fun localIpAddress(app: Application): String? {
        val connectivity = app.getSystemService(ConnectivityManager::class.java)
        val fromActiveNetwork = connectivity.activeNetwork
            ?.let { connectivity.getLinkProperties(it) }
            ?.linkAddresses
            ?.map { it.address }
            ?.firstOrNull(::isLanIpv4)
        if (fromActiveNetwork != null) return fromActiveNetwork.hostAddress

        return runCatching {
            NetworkInterface.getNetworkInterfaces().toList()
                .filter { it.isUp && !it.isLoopback }
                .flatMap { it.inetAddresses.toList() }
                .firstOrNull(::isLanIpv4)
                ?.hostAddress
        }.getOrNull()
    }

    private fun isLanIpv4(address: InetAddress) = address is Inet4Address && !address.isLoopbackAddress
}
