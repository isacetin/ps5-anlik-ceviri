package com.isacetin.ps5ceviri.overlay

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class HistoryPlacementTest {

    /** Ekran yüksekliğine göre alt-ortadaki altyazı kutusu (1080p'de 576x86). */
    private fun anchor(screenWidth: Int, screenHeight: Int) =
        PixelRect(screenWidth * 0.35f, screenHeight * 0.8f, screenWidth * 0.3f, screenHeight * 0.08f)

    private fun place(screenWidth: Int, screenHeight: Int, count: Int = 2, others: List<PixelRect> = emptyList(), anchor: PixelRect = anchor(screenWidth, screenHeight)) =
        placeHistory(
            anchor = anchor,
            sizes = List(count) { anchor.width * 0.8f to screenHeight * 0.05f },
            gap = screenHeight * STACK_GAP_RATIO,
            topLimit = historyTopLimit(anchor.top, screenHeight),
            others = others,
            screenWidth = screenWidth,
        )

    @Test
    fun `kutular capanin ustune ortali ve ust uste dizilir`() {
        val a = anchor(1920, 1080)
        val placed = place(1920, 1080)

        assertEquals(2, placed.size)
        val gap = 1080 * STACK_GAP_RATIO
        assertEquals(a.top - gap, placed[0].top + placed[0].height, 0.01f)
        assertEquals(placed[0].top - gap, placed[1].top + placed[1].height, 0.01f)
        placed.forEach { assertEquals(a.left + a.width / 2, it.left + it.width / 2, 0.01f) }
    }

    @Test
    fun `farkli cozunurluklerde ayni oranlarla yerlesir`() {
        for ((w, h) in listOf(1280 to 720, 1920 to 1080, 3840 to 2160)) {
            val placed = place(w, h)
            assertEquals("$w x $h", 2, placed.size)
            assertEquals(placed[1].top / h, place(1920, 1080)[1].top / 1080, 0.001f)
        }
    }

    @Test
    fun `ust guvenli pay ve yigin yuksekligi asilmaz`() {
        // Altyazı ekranın üstündeyse yığın güvenli payın üstüne çıkmaz: sığmayan satır çizilmez.
        val high = PixelRect(600f, 1080 * 0.09f, 700f, 60f)
        val placed = place(1920, 1080, anchor = high)
        assertTrue(placed.size < 2)
        placed.forEach { assertTrue(it.top >= 1080 * SAFE_TOP_RATIO) }
        // Çok satır olsa da yığın ekranın %22'sini aşmaz.
        val many = place(1920, 1080, count = 10)
        val a = anchor(1920, 1080)
        assertTrue(many.isNotEmpty() && many.size < 10)
        many.forEach { assertTrue(it.top >= a.top - 1080 * MAX_STACK_RATIO) }
    }

    @Test
    fun `baska ceviri kutusuyla cakisan satir ve ustundekiler cizilmez`() {
        val a = anchor(1920, 1080)
        val hud = PixelRect(a.left, a.top - 140f, a.width, 40f)
        val placed = place(1920, 1080, others = listOf(hud))

        assertEquals(1, placed.size)
        assertTrue(placed[0].top + placed[0].height <= a.top)
        // Yandaki kutu çakışmıyorsa engel olmaz.
        val side = PixelRect(10f, a.top - 300f, 200f, 300f)
        assertEquals(2, place(1920, 1080, others = listOf(side)).size)
    }

    @Test
    fun `ekran kenarindaki capada kutu ekrandan tasmaz`() {
        val edge = PixelRect(0f, 900f, 200f, 60f)
        val placed = placeHistory(edge, listOf(400f to 50f), 6f, historyTopLimit(edge.top, 1080), emptyList(), 1920)

        assertEquals(0f, placed.single().left)
        val wide = placeHistory(edge, listOf(5000f to 50f), 6f, historyTopLimit(edge.top, 1080), emptyList(), 1920)
        assertEquals(1920f, wide.single().width)
    }
}
