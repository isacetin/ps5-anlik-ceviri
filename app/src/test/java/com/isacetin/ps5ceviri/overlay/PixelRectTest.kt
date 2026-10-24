package com.isacetin.ps5ceviri.overlay

import com.isacetin.ps5ceviri.network.Block
import org.junit.Assert.assertEquals
import org.junit.Test

class PixelRectTest {

    @Test
    fun `normalize koordinat 1920x1080 ekranda piksele cevrilir`() {
        val block = Block(id = "b", x = 0.25f, y = 0.5f, w = 0.5f, h = 0.1f, text = "x")

        val rect = block.toPixelRect(1920, 1080)

        assertEquals(480f, rect.left, 0.01f)
        assertEquals(540f, rect.top, 0.01f)
        assertEquals(960f, rect.width, 0.01f)
        assertEquals(108f, rect.height, 0.01f)
    }

    @Test
    fun `origin sol ust kosedir`() {
        val rect = Block(id = "b", x = 0f, y = 0f, w = 1f, h = 1f, text = "x").toPixelRect(3840, 2160)

        assertEquals(PixelRect(0f, 0f, 3840f, 2160f), rect)
    }

    @Test
    fun `genisletilen kutu merkezini korur`() {
        val rect = PixelRect(left = 800f, top = 500f, width = 200f, height = 40f)

        val widened = rect.widenedTo(400f, 1920)

        assertEquals(PixelRect(700f, 500f, 400f, 40f), widened)
    }

    @Test
    fun `genisletilen kutu ekrandan tasmaz`() {
        val nearRightEdge = PixelRect(left = 1800f, top = 0f, width = 100f, height = 40f)
        val nearLeftEdge = PixelRect(left = 10f, top = 0f, width = 100f, height = 40f)

        assertEquals(PixelRect(1520f, 0f, 400f, 40f), nearRightEdge.widenedTo(400f, 1920))
        assertEquals(PixelRect(0f, 0f, 400f, 40f), nearLeftEdge.widenedTo(400f, 1920))
    }

    @Test
    fun `kutu daraltilmaz ve ekrandan genis olamaz`() {
        val rect = PixelRect(left = 100f, top = 0f, width = 500f, height = 40f)

        assertEquals(rect, rect.widenedTo(300f, 1920))
        assertEquals(1920f, rect.widenedTo(5000f, 1920).width)
    }
}
