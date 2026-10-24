package com.isacetin.ps5ceviri.overlay

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.graphics.PixelFormat
import android.hardware.display.DisplayManager
import android.os.Build
import android.os.IBinder
import android.provider.Settings
import android.util.DisplayMetrics
import android.util.Log
import android.view.Display
import android.view.Gravity
import android.view.WindowManager
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.edit
import com.isacetin.ps5ceviri.R
import com.isacetin.ps5ceviri.discovery.NsdAdvertiser
import com.isacetin.ps5ceviri.network.WebSocketServer
import com.isacetin.ps5ceviri.ui.MainActivity
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch

/**
 * Overlay penceresini, WebSocket server'ı ve NSD yayınını ayakta tutan foreground service.
 * Overlay izni yoksa servis (ve server) çalışır ama pencere oluşturmaz.
 */
class OverlayService : Service() {

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private lateinit var windowManager: WindowManager
    private var overlayView: TranslationOverlayView? = null
    private var renderJob: Job? = null
    private var server: WebSocketServer? = null
    private lateinit var nsdAdvertiser: NsdAdvertiser

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        val overlayContext = overlayWindowContext()
        windowManager = overlayContext.getSystemService(WindowManager::class.java)
        nsdAdvertiser = NsdAdvertiser(this)
        createNotificationChannel()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (!startInForeground()) {
            stopSelf()
            return START_NOT_STICKY
        }
        // Tekrar başlatma çağrısı (ör. izin sonradan verildiyse) overlay'i eklemeyi yeniden dener.
        attachOverlayIfAllowed()
        startServer()
        OverlayState.updateStatus { it.copy(serviceRunning = true, overlayAttached = overlayView != null) }
        return START_STICKY
    }

    override fun onDestroy() {
        detachOverlay()
        nsdAdvertiser.unregister()
        server?.stop()
        server = null
        scope.cancel()
        OverlayState.updateStatus { ServiceStatus() }
        super.onDestroy()
    }

    private fun startInForeground(): Boolean {
        val openApp = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE,
        )
        val notification = NotificationCompat.Builder(this, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(getString(R.string.notification_title))
            .setContentText(getString(R.string.notification_text, WS_PORT))
            .setContentIntent(openApp)
            .setOngoing(true)
            .build()
        val type = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE
        } else {
            0
        }
        return try {
            ServiceCompat.startForeground(this, NOTIFICATION_ID, notification, type)
            true
        } catch (e: IllegalStateException) {
            // Android 12+: arka plandan izin verilmeyen bir başlatma (ForegroundServiceStartNotAllowedException).
            Log.w(TAG, "Foreground service başlatılamadı", e)
            false
        }
    }

    private fun startServer() {
        if (server != null) return
        val newServer = WebSocketServer(WS_PORT)
        server = newServer
        scope.launch(Dispatchers.IO) { newServer.start() }
        nsdAdvertiser.register(WS_PORT)
    }

    private fun attachOverlayIfAllowed() {
        if (overlayView != null) return
        if (!Settings.canDrawOverlays(this)) {
            Log.w(TAG, "Overlay izni yok; overlay oluşturulmadı")
            return
        }

        val view = TranslationOverlayView(this, ::screenSize)
        val params = WindowManager.LayoutParams(
            WindowManager.LayoutParams.MATCH_PARENT,
            WindowManager.LayoutParams.MATCH_PARENT,
            WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
            WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or
                WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE or
                WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN,
            PixelFormat.TRANSLUCENT,
        ).apply {
            gravity = Gravity.TOP or Gravity.START
            title = "PS5CeviriOverlay"
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                fitInsetsTypes = 0
                layoutInDisplayCutoutMode = WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_ALWAYS
            } else if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                layoutInDisplayCutoutMode = WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES
            }
        }

        try {
            windowManager.addView(view, params)
        } catch (e: RuntimeException) {
            // BadTokenException / SecurityException: izin tam o anda geri alınmış olabilir.
            Log.e(TAG, "Overlay penceresi eklenemedi", e)
            return
        }
        overlayView = view
        val (width, height) = screenSize()
        Log.i(TAG, "Overlay eklendi, ekran ${width}x$height")

        renderJob = scope.launch {
            OverlayState.blocks.collect { view.setBlocks(it) }
        }
    }

    private fun detachOverlay() {
        renderJob?.cancel()
        renderJob = null
        overlayView?.let { view ->
            try {
                windowManager.removeView(view)
            } catch (e: IllegalArgumentException) {
                Log.w(TAG, "Overlay zaten kaldırılmış", e)
            }
        }
        overlayView = null
    }

    /** Gerçek ekran boyutu; TV'lerde resources.displayMetrics farklı (ölçeklenmiş) değer dönebilir. */
    private fun screenSize(): Pair<Int, Int> =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            val bounds = windowManager.currentWindowMetrics.bounds
            bounds.width() to bounds.height()
        } else {
            val metrics = DisplayMetrics()
            @Suppress("DEPRECATION")
            windowManager.defaultDisplay.getRealMetrics(metrics)
            metrics.widthPixels to metrics.heightPixels
        }

    /** Android 11+'da overlay pencereleri için önerilen yol: overlay tipine bağlı bir window context. */
    private fun overlayWindowContext(): Context =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            val display = getSystemService(DisplayManager::class.java).getDisplay(Display.DEFAULT_DISPLAY)
            createDisplayContext(display).createWindowContext(WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY, null)
        } else {
            this
        }

    private fun createNotificationChannel() {
        val channel = NotificationChannel(
            CHANNEL_ID,
            getString(R.string.notification_channel_name),
            NotificationManager.IMPORTANCE_LOW,
        )
        getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    }

    companion object {
        const val WS_PORT = 8765

        private const val TAG = "OverlayService"
        private const val CHANNEL_ID = "overlay"
        private const val NOTIFICATION_ID = 1
        private const val PREFS = "overlay"
        private const val KEY_START_ON_BOOT = "start_on_boot"

        /** Servisi başlatır ve TV açılışında otomatik başlamayı açar. */
        fun start(context: Context) {
            context.getSharedPreferences(PREFS, MODE_PRIVATE).edit { putBoolean(KEY_START_ON_BOOT, true) }
            context.startForegroundService(Intent(context, OverlayService::class.java))
        }

        /** Servisi durdurur; kullanıcı durdurduysa açılışta da başlamaz. */
        fun stop(context: Context) {
            context.getSharedPreferences(PREFS, MODE_PRIVATE).edit { putBoolean(KEY_START_ON_BOOT, false) }
            context.stopService(Intent(context, OverlayService::class.java))
        }

        fun shouldStartOnBoot(context: Context): Boolean =
            context.getSharedPreferences(PREFS, MODE_PRIVATE).getBoolean(KEY_START_ON_BOOT, false)
    }
}
