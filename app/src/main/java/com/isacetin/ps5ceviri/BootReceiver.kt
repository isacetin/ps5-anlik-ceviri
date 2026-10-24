package com.isacetin.ps5ceviri

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log
import com.isacetin.ps5ceviri.overlay.OverlayService

/**
 * TV açılınca ve uygulama güncellenince (adb install -r) servisi başlatır (kullanıcı son olarak
 * durdurmadıysa). Güncelleme süreci öldürür; bu olmadan overlay elle yeniden açılmak zorunda kalırdı.
 * BOOT_COMPLETED ve MY_PACKAGE_REPLACED, Android 12+ arka plan FGS kısıtından muaftır; Android 15'in açılışta
 * yasakladığı tipler (dataSync, mediaPlayback, camera, ...) arasında specialUse yoktur.
 * Overlay izni yoksa servis yine başlar ama pencere oluşturmaz.
 */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_BOOT_COMPLETED && intent.action != Intent.ACTION_MY_PACKAGE_REPLACED) return
        if (!OverlayService.shouldStartOnBoot(context)) return
        try {
            context.startForegroundService(Intent(context, OverlayService::class.java))
        } catch (e: IllegalStateException) {
            Log.w("BootReceiver", "Açılışta servis başlatılamadı", e)
        }
    }
}
