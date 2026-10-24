package com.isacetin.ps5ceviri.discovery

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.util.Log

/**
 * WebSocket server'ı yerel ağda _ps5ceviri._tcp olarak yayınlar; Mac IP bilmeden bulabilsin.
 * Yayın başarısız olsa da IP ile manuel bağlantı çalışmaya devam eder.
 */
class NsdAdvertiser(context: Context) {

    private val nsdManager = context.getSystemService(NsdManager::class.java)
    private var listener: NsdManager.RegistrationListener? = null

    fun register(port: Int) {
        if (listener != null) return
        val info = NsdServiceInfo().apply {
            serviceName = SERVICE_NAME
            serviceType = SERVICE_TYPE
            setPort(port)
        }
        val registrationListener = object : NsdManager.RegistrationListener {
            override fun onServiceRegistered(info: NsdServiceInfo) {
                Log.i(TAG, "NSD yayında: ${info.serviceName}")
            }

            override fun onRegistrationFailed(info: NsdServiceInfo, errorCode: Int) {
                Log.w(TAG, "NSD kaydı başarısız: $errorCode")
            }

            override fun onServiceUnregistered(info: NsdServiceInfo) {
                Log.i(TAG, "NSD yayını kaldırıldı")
            }

            override fun onUnregistrationFailed(info: NsdServiceInfo, errorCode: Int) {
                Log.w(TAG, "NSD kaydı kaldırılamadı: $errorCode")
            }
        }
        try {
            nsdManager.registerService(info, NsdManager.PROTOCOL_DNS_SD, registrationListener)
            listener = registrationListener
        } catch (e: RuntimeException) {
            Log.w(TAG, "NSD kaydı yapılamadı", e)
        }
    }

    fun unregister() {
        val current = listener ?: return
        listener = null
        try {
            nsdManager.unregisterService(current)
        } catch (e: IllegalArgumentException) {
            Log.w(TAG, "NSD kaydı zaten kaldırılmış", e)
        }
    }

    private companion object {
        const val TAG = "NsdAdvertiser"
        const val SERVICE_NAME = "PS5 Ceviri"
        const val SERVICE_TYPE = "_ps5ceviri._tcp"
    }
}
