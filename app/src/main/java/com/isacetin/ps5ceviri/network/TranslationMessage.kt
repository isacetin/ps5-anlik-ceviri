package com.isacetin.ps5ceviri.network

import kotlinx.serialization.SerializationException
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json

/**
 * Ekrandaki tek bir çeviri bloğu. Koordinatlar 0..1 aralığında normalize, origin sol üst.
 * [lineH] orijinal satır yüksekliğidir ve font boyutu için başlangıç değeri olarak kullanılır.
 */
@Serializable
data class Block(
    val id: String,
    val x: Float,
    val y: Float,
    val w: Float,
    val h: Float,
    val lineH: Float = 0f,
    val text: String,
)

/** Mac → TV mesajı. [blocks] ekranda olması gereken listenin tamamıdır. */
@Serializable
data class TranslationMessage(
    val v: Int = 1,
    val seq: Long,
    val blocks: List<Block>,
)

private val json = Json { ignoreUnknownKeys = true }

/** Geçersiz veya bozuk JSON'da null döner; çağıran taraf loglayıp mesajı yok sayar. */
fun parseTranslationMessage(raw: String): TranslationMessage? =
    try {
        json.decodeFromString<TranslationMessage>(raw)
    } catch (e: SerializationException) {
        null
    } catch (e: IllegalArgumentException) {
        null
    }

/**
 * Tek bir WebSocket bağlantısının seq kuralı: son kabul edilenden küçük seq'li mesajlar yok
 * sayılır. Her yeni bağlantı kendi örneğini oluşturur, yani sayaç bağlantıyla sıfırlanır.
 */
class SeqGate {
    private var lastAccepted: Long? = null

    fun accept(seq: Long): Boolean {
        val last = lastAccepted
        if (last != null && seq < last) return false
        lastAccepted = seq
        return true
    }
}
