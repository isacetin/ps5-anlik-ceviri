package com.isacetin.ps5ceviri.ui

import android.content.ActivityNotFoundException
import android.content.Intent
import android.os.Bundle
import android.provider.Settings
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.viewModels
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.core.net.toUri
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.tv.material3.Button
import androidx.tv.material3.MaterialTheme
import androidx.tv.material3.Surface
import androidx.tv.material3.SurfaceDefaults
import androidx.tv.material3.Text
import androidx.tv.material3.darkColorScheme
import com.isacetin.ps5ceviri.overlay.OverlayService

class MainActivity : ComponentActivity() {

    private val viewModel: MainViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        handleSetupIntent(intent)
        setContent {
            MaterialTheme(colorScheme = darkColorScheme()) {
                val state by viewModel.uiState.collectAsStateWithLifecycle()
                MainScreen(
                    state = state,
                    adbCommand = "adb shell appops set $packageName SYSTEM_ALERT_WINDOW allow",
                    onToggleOverlay = viewModel::toggleOverlay,
                    onShowTest = viewModel::showTestText,
                    onClearTest = viewModel::clearTestText,
                    onOpenPermissionSettings = ::openOverlayPermissionSettings,
                )
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleSetupIntent(intent)
    }

    override fun onResume() {
        super.onResume()
        viewModel.refresh()
    }

    /**
     * Masaüstü uygulamasının kurulum sihirbazı, kurulumdan sonra uygulamayı
     * `am start -n <paket>/.ui.MainActivity --ez start_overlay true` ile açar: overlay kullanıcı
     * TV'de düğmeye basmadan başlar (servis dışarıya kapalı kalır; başlatan bu ekrandır).
     */
    private fun handleSetupIntent(intent: Intent?) {
        if (intent?.getBooleanExtra(EXTRA_START_OVERLAY, false) == true) {
            OverlayService.start(this)
            intent.removeExtra(EXTRA_START_OVERLAY)
        }
    }

    companion object {
        const val EXTRA_START_OVERLAY = "start_overlay"
    }

    /** Birçok Android TV'de bu ekran yoktur; açılamazsa çökmek yerine adb komutuna yönlendiririz. */
    private fun openOverlayPermissionSettings() {
        val intents = listOf(
            Intent(Settings.ACTION_MANAGE_OVERLAY_PERMISSION, "package:$packageName".toUri()),
            Intent(Settings.ACTION_MANAGE_OVERLAY_PERMISSION),
        )
        for (intent in intents) {
            try {
                startActivity(intent)
                return
            } catch (e: ActivityNotFoundException) {
                // sıradakini dene
            } catch (e: SecurityException) {
                // sıradakini dene
            }
        }
        viewModel.onPermissionScreenUnavailable()
    }
}

private val Green = Color(0xFF5BD47A)
private val Red = Color(0xFFFF6B6B)

@Composable
private fun MainScreen(
    state: MainUiState,
    adbCommand: String,
    onToggleOverlay: () -> Unit,
    onShowTest: () -> Unit,
    onClearTest: () -> Unit,
    onOpenPermissionSettings: () -> Unit,
) {
    Surface(
        modifier = Modifier.fillMaxSize(),
        shape = RectangleShape,
        colors = SurfaceDefaults.colors(containerColor = Color(0xFF0B1220), contentColor = Color.White),
    ) {
        // TV overscan için kenarlarda güvenli boşluk (48dp yatay, 27dp dikey).
        Column(Modifier.fillMaxSize().padding(horizontal = 48.dp, vertical = 27.dp)) {
            Text("PS5 Çeviri", style = MaterialTheme.typography.displaySmall, fontWeight = FontWeight.Bold)
            Text(
                "PS5 görüntüsünün üzerine Türkçe çeviri overlay'i",
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(28.dp))

            Row(horizontalArrangement = Arrangement.spacedBy(48.dp)) {
                Column(Modifier.weight(1.5f), verticalArrangement = Arrangement.spacedBy(14.dp)) {
                    StatusRow(
                        label = "Bağlantı adresi",
                        value = state.ipAddress?.let { "ws://$it:${state.port}" } ?: "Ağ bulunamadı (port ${state.port})",
                        ok = state.ipAddress != null,
                    )
                    StatusRow(
                        label = "Overlay izni",
                        value = if (state.overlayPermission) "Verildi" else "Overlay izni verilmedi",
                        ok = state.overlayPermission,
                    )
                    StatusRow(
                        label = "Servis",
                        value = if (state.status.serviceRunning) "Çalışıyor" else "Durdu",
                        ok = state.status.serviceRunning,
                    )
                    StatusRow(
                        label = "Overlay penceresi",
                        value = if (state.status.overlayAttached) "Ekranda" else "Yok",
                        ok = state.status.overlayAttached,
                    )
                    StatusRow(
                        label = "WebSocket server",
                        value = if (state.status.serverRunning) {
                            "Açık · ${state.status.clientCount} istemci bağlı"
                        } else {
                            "Kapalı"
                        },
                        ok = state.status.serverRunning,
                    )
                    if (!state.overlayPermission) {
                        PermissionHelp(adbCommand, state.message)
                    }
                }

                ActionButtons(
                    modifier = Modifier.weight(1f),
                    state = state,
                    onToggleOverlay = onToggleOverlay,
                    onShowTest = onShowTest,
                    onClearTest = onClearTest,
                    onOpenPermissionSettings = onOpenPermissionSettings,
                )
            }
        }
    }
}

@Composable
private fun StatusRow(label: String, value: String, ok: Boolean) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text(
            label,
            modifier = Modifier.width(220.dp),
            style = MaterialTheme.typography.titleMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            value,
            style = MaterialTheme.typography.titleLarge,
            fontWeight = FontWeight.SemiBold,
            color = if (ok) Green else Red,
        )
    }
}

@Composable
private fun PermissionHelp(adbCommand: String, message: String?) {
    Surface(
        shape = RoundedCornerShape(12.dp),
        colors = SurfaceDefaults.colors(containerColor = Color(0xFF2A1A1A), contentColor = Color.White),
    ) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text("Overlay izni verilmedi", style = MaterialTheme.typography.titleMedium, color = Red)
            message?.let { Text(it, style = MaterialTheme.typography.bodyLarge) }
            Text("Bilgisayardan şu komutu çalıştır:", style = MaterialTheme.typography.bodyLarge)
            Text(adbCommand, style = MaterialTheme.typography.bodyLarge, fontFamily = FontFamily.Monospace)
        }
    }
}

@Composable
private fun ActionButtons(
    modifier: Modifier,
    state: MainUiState,
    onToggleOverlay: () -> Unit,
    onShowTest: () -> Unit,
    onClearTest: () -> Unit,
    onOpenPermissionSettings: () -> Unit,
) {
    val firstButton = remember { FocusRequester() }
    LaunchedEffect(Unit) { runCatching { firstButton.requestFocus() } }

    Column(modifier, verticalArrangement = Arrangement.spacedBy(12.dp)) {
        TvButton(
            text = if (state.status.serviceRunning) "Overlay'i durdur" else "Overlay'i başlat",
            onClick = onToggleOverlay,
            modifier = Modifier.focusRequester(firstButton),
        )
        TvButton("Test yazısı göster", onShowTest)
        TvButton("Test yazısını kaldır", onClearTest)
        if (!state.overlayPermission) {
            TvButton("İzin ekranını aç", onOpenPermissionSettings)
        }
    }
}

@Composable
private fun TvButton(text: String, onClick: () -> Unit, modifier: Modifier = Modifier) {
    Button(
        onClick = onClick,
        modifier = modifier.fillMaxWidth(),
        contentPadding = PaddingValues(horizontal = 24.dp, vertical = 14.dp),
    ) {
        Text(text, style = MaterialTheme.typography.titleMedium)
    }
}
