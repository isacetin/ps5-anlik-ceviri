package com.isacetin.ps5ceviri.overlay

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class FontFitterTest {

    /** Basit model: her karakter fontSize * 0.6 genişliğinde, tek satır, yükseklik fontSize * 1.2. */
    private val singleLine = { text: String, size: Float, _: Float -> TextSize(text.length * size * 0.6f, size * 1.2f) }

    @Test
    fun `sigan metin baslangic boyutunu korur`() {
        val size = FontFitter.fit(
            text = "KAPI KİLİTLİ",
            boxWidth = 960f,
            boxHeight = 108f,
            initialSize = 43f,
            minSize = 16f,
            maxSize = 130f,
            measure = singleLine,
        )
        assertEquals(43f, size, 0.001f)
    }

    @Test
    fun `sigmayan metin kucultulur`() {
        // 20 karakter: genişlik = 20 * 0.6 * size <= 300 için size <= 25.
        val size = FontFitter.fit(
            text = "a".repeat(20),
            boxWidth = 300f,
            boxHeight = 100f,
            initialSize = 40f,
            minSize = 10f,
            maxSize = 100f,
            measure = singleLine,
        )
        assertEquals(25f, size, 0.001f)
    }

    @Test
    fun `minimum font sinirinin altina inmez`() {
        val size = FontFitter.fit(
            text = "çok uzun bir Türkçe cümle ğüşıöç İĞÜŞÖÇ".repeat(10),
            boxWidth = 100f,
            boxHeight = 20f,
            initialSize = 40f,
            minSize = 16f,
            maxSize = 100f,
            measure = singleLine,
        )
        assertEquals(16f, size, 0.001f)
    }

    @Test
    fun `baslangic boyutu maksimuma kirpilir`() {
        val size = FontFitter.fit(
            text = "A",
            boxWidth = 1000f,
            boxHeight = 1000f,
            initialSize = 500f,
            minSize = 10f,
            maxSize = 80f,
            measure = singleLine,
        )
        assertEquals(80f, size, 0.001f)
    }

    @Test
    fun `yukseklik sigmazsa da kucultulur`() {
        // Satır kaydırma modeli: genişlik sınırlı, yükseklik satır sayısıyla büyür.
        val wrapping = { text: String, size: Float, maxWidth: Float ->
            val lineWidth = text.length * size * 0.6f
            val lines = kotlin.math.ceil(lineWidth / maxWidth).coerceAtLeast(1f)
            TextSize(minOf(lineWidth, maxWidth), lines * size * 1.2f)
        }
        val size = FontFitter.fit(
            text = "a".repeat(40),
            boxWidth = 240f,
            boxHeight = 60f,
            initialSize = 40f,
            minSize = 8f,
            maxSize = 100f,
            measure = wrapping,
        )
        val measured = wrapping("a".repeat(40), size, 240f)
        assertTrue("yükseklik sığmalı: ${measured.height}", measured.height <= 60f)
        assertTrue(size < 40f)
    }
}
