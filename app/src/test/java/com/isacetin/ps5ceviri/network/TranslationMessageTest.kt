package com.isacetin.ps5ceviri.network

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class TranslationMessageTest {

    @Test
    fun `gecerli mesaj parse edilir`() {
        val message = parseTranslationMessage(
            """
            {
              "v": 1,
              "seq": 123,
              "blocks": [
                { "id": "abc123", "x": 0.12, "y": 0.81, "w": 0.60, "h": 0.08, "lineH": 0.03, "text": "KAPI KİLİTLİ" }
              ]
            }
            """.trimIndent(),
        )

        requireNotNull(message)
        assertEquals(1, message.v)
        assertEquals(123L, message.seq)
        assertEquals(
            Block(id = "abc123", x = 0.12f, y = 0.81f, w = 0.60f, h = 0.08f, lineH = 0.03f, text = "KAPI KİLİTLİ"),
            message.blocks.single(),
        )
    }

    @Test
    fun `turkce karakterler korunur`() {
        val text = "çğıİöşü ÇĞÖŞÜ"
        val message = parseTranslationMessage(
            """{"v":1,"seq":1,"blocks":[{"id":"t","x":0,"y":0,"w":1,"h":1,"lineH":0.1,"text":"$text"}]}""",
        )

        assertEquals(text, message?.blocks?.single()?.text)
    }

    @Test
    fun `bilinmeyen alanlar hata vermez`() {
        val message = parseTranslationMessage(
            """
            {"v":1,"seq":5,"mode":"POSITION","source":"mac",
             "blocks":[{"id":"a","x":0.1,"y":0.2,"w":0.3,"h":0.4,"lineH":0.05,"text":"Merhaba","confidence":0.97}]}
            """.trimIndent(),
        )

        requireNotNull(message)
        assertEquals("Merhaba", message.blocks.single().text)
    }

    @Test
    fun `bos blocks listesi gecerlidir`() {
        val message = parseTranslationMessage("""{"v":1,"seq":9,"blocks":[]}""")

        requireNotNull(message)
        assertTrue(message.blocks.isEmpty())
    }

    @Test
    fun `bozuk json null doner`() {
        assertNull(parseTranslationMessage("""{"v":1,"seq":"""))
        assertNull(parseTranslationMessage("bu json değil"))
        assertNull(parseTranslationMessage(""))
    }

    @Test
    fun `zorunlu alan eksikse veya tip yanlissa null doner`() {
        assertNull(parseTranslationMessage("""{"v":1,"blocks":[]}"""))
        assertNull(parseTranslationMessage("""{"v":1,"seq":1,"blocks":[{"id":"a","x":"sol","y":0,"w":1,"h":1,"text":"x"}]}"""))
    }
}
