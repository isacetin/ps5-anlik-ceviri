package com.isacetin.ps5ceviri.overlay

import com.isacetin.ps5ceviri.network.Block
import kotlin.math.min

/** Masaüstünün altyazı bloklarına verdiği tür (`Block.kind`). */
const val SUBTITLE_KIND = "subtitle"

/** Güncel altyazının üstünde çizilecek önceki bir altyazı. [level] 1: hemen üstte, 2: onun üstünde. */
data class HistoryLine(val text: String, val level: Int, val alpha: Float, val scale: Float)

/** Çizim anındaki yığın: önceki satırlar [anchor] altyazısının üstüne dizilir. */
data class StackSnapshot(val anchor: Block?, val lines: List<HistoryLine>) {
    companion object {
        val EMPTY = StackSnapshot(null, emptyList())
    }
}

/**
 * Hızlı konuşmada önceki altyazı satırları. Yeni altyazı geldiğinde önceki satır okunacak kadar
 * ekranda kalmadıysa yeni satırın üstüne küçük ve soluk olarak çıkar, bir süre sonra solarak
 * kalkar; böylece hızlı diyalogda satır okunmadan kaybolmaz. Ekranda en çok [maxSegments] altyazı
 * (güncel + önceki) olur.
 *
 * Yalnızca masaüstünün `kind = "subtitle"` diye işaretlediği bloklar yığına girer; alanı
 * göndermeyen eski masaüstü sürümünde yığın hiç kurulmaz (eski davranış).
 *
 * Bayat satır gösterilmez:
 * - Önceki satır [holdMillis] sonra solmaya başlar, [fadeMillis] sonra kalkar.
 * - Altyazı [sceneGapMillis]'ten uzun süre ekranda yoksa (yeni sahne, duraklatma, bağlantı
 *   kopması) yığın boşalır; sonra gelen satırın üstüne eski satırlar çıkmaz.
 * - Önceki satır okunacak kadar ekranda kaldıysa ([readingMillis]) yığına girmez ve süreklilik
 *   koptuğu için eski satırlar da atılır.
 *
 * Zaman dışarıdan verilir (milisaniye; Android'de uyku süresini de sayan `elapsedRealtime`), testler
 * saati kendisi ilerletir. Ağ thread'i günceller, çizim thread'i okur: yöntemler senkronizedir.
 */
class SubtitleStack(
    private val maxSegments: Int = 3,
    private val holdMillis: Long = 4_000,
    private val fadeMillis: Long = 600,
    private val sceneGapMillis: Long = 1_500,
) {
    private class Shown(var block: Block, val shownAt: Long)
    private class Past(val text: String, val replacedAt: Long)

    private var current: Shown? = null
    /** Ekrandan kalkan son altyazı ve kalktığı an (yeni satır hemen gelirse yığına girer). */
    private var gone: Shown? = null
    private var goneAt = 0L
    /** En yenisi başta. */
    private val history = ArrayDeque<Past>()

    /** Her mesajda çağrılır: ekranda olması gereken blokların tamamı. */
    @Synchronized
    fun update(blocks: List<Block>, now: Long) {
        prune(now)
        val subtitle = blocks.filter { it.kind == SUBTITLE_KIND && it.text.isNotBlank() }.maxByOrNull { it.y + it.h }
        val shown = current
        if (subtitle == null) {
            if (shown != null) {
                gone = shown
                goneAt = now
                current = null
            }
            return
        }
        if (shown != null && sameLine(shown.block.text, subtitle.text)) {
            shown.block = subtitle
            return
        }
        // Bir anlık okuma kesintisinden sonra aynı satır geri geldi: yığın değişmez.
        val returning = gone?.takeIf { shown == null && now - goneAt <= sceneGapMillis && sameLine(it.block.text, subtitle.text) }
        if (returning != null) {
            returning.block = subtitle
            current = returning
            gone = null
            return
        }
        // Yeni satır. Önceki: ekrandaki ya da az önce kalkan altyazı.
        val previous = shown ?: gone?.takeIf { now - goneAt <= sceneGapMillis }
        val visibleFor = when {
            previous == null -> Long.MAX_VALUE
            previous === shown -> now - previous.shownAt
            else -> goneAt - previous.shownAt
        }
        if (previous != null && visibleFor < readingMillis(previous.block.text) && !sameLine(previous.block.text, subtitle.text)) {
            history.addFirst(Past(previous.block.text, now))
            while (history.size > maxSegments - 1) history.removeLast()
        } else {
            history.clear()
        }
        current = Shown(subtitle, now)
        gone = null
    }

    /** Bağlantı koptu, ekran temizlendi ya da çeviri durduruldu: yığın hemen boşalır. */
    @Synchronized
    fun reset() {
        current = null
        gone = null
        history.clear()
    }

    /** [now] anında çizilecek önceki satırlar. */
    @Synchronized
    fun snapshot(now: Long): StackSnapshot {
        val anchor = current?.block ?: gone?.takeIf { now - goneAt <= sceneGapMillis }?.block ?: return StackSnapshot.EMPTY
        val lines = history.mapIndexedNotNull { index, past ->
            val age = now - past.replacedAt
            if (age >= holdMillis + fadeMillis || age < 0) return@mapIndexedNotNull null
            val fade = if (age <= holdMillis) 1f else 1f - (age - holdMillis).toFloat() / fadeMillis
            val level = index + 1
            HistoryLine(past.text, level, alphaFor(level) * fade, scaleFor(level))
        }
        return if (lines.isEmpty()) StackSnapshot.EMPTY else StackSnapshot(anchor, lines)
    }

    /** Görünümün bir sonraki değişimine kalan süre (ms); solma sürerken 0, değişecek bir şey yoksa null. */
    @Synchronized
    fun nextChangeIn(now: Long): Long? {
        val times = mutableListOf<Long>()
        for (past in history) {
            val age = now - past.replacedAt
            when {
                age < holdMillis -> times += holdMillis - age
                age < holdMillis + fadeMillis -> times += 0L
            }
        }
        if (current == null && gone != null && history.isNotEmpty()) {
            val left = sceneGapMillis - (now - goneAt)
            if (left > 0) times += left
        }
        return times.minOrNull()
    }

    private fun prune(now: Long) {
        history.removeAll { now - it.replacedAt >= holdMillis + fadeMillis }
        if (current == null && gone != null && now - goneAt > sceneGapMillis) {
            gone = null
            history.clear()
        }
    }

    companion object {
        /** Bir satırı okumak için gereken süre: 1 sn + harf başına 50 ms, en çok 5 sn. */
        fun readingMillis(text: String): Long = min(1_000L + 50L * text.length, 5_000L)

        fun alphaFor(level: Int): Float = if (level <= 1) 0.7f else 0.45f

        fun scaleFor(level: Int): Float = if (level <= 1) 0.8f else 0.68f

        /**
         * Aynı satırın yeniden okuması ya da uzaması (harf harf yazılan altyazı): boşluk ve
         * büyük/küçük harf farkı yok sayılır, biri öbürünün başıysa aynı satırdır.
         */
        fun sameLine(a: String, b: String): Boolean {
            val x = normalize(a)
            val y = normalize(b)
            if (x.isEmpty() || y.isEmpty()) return x == y
            return x == y || x.startsWith(y) || y.startsWith(x)
        }

        private fun normalize(text: String) = text.filter { it.isLetterOrDigit() }.lowercase()
    }
}
