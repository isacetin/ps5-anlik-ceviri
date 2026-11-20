package com.isacetin.ps5ceviri.overlay

import com.isacetin.ps5ceviri.network.Block
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

class SubtitleStackTest {

    private fun subtitle(text: String, id: String = "s1") =
        Block(id = id, x = 0.3f, y = 0.82f, w = 0.4f, h = 0.06f, lineH = 0.04f, text = text, kind = SUBTITLE_KIND)

    private val menu = Block(id = "m1", x = 0.05f, y = 0.1f, w = 0.2f, h = 0.04f, lineH = 0.03f, text = "Envanter")

    private fun SubtitleStack.texts(now: Long) = snapshot(now).lines.map { it.text }

    @Test
    fun `tek altyazida yigin bos kalir`() {
        val stack = SubtitleStack()
        stack.update(listOf(subtitle("Gitmemiz gerek.")), 0)

        assertTrue(stack.snapshot(500).lines.isEmpty())
        assertNull(stack.nextChangeIn(500))
    }

    @Test
    fun `hizli konusmada onceki satir ustte kucuk ve soluk kalir`() {
        val stack = SubtitleStack()
        stack.update(listOf(subtitle("Gitmemiz gerek.")), 0)
        stack.update(listOf(subtitle("Neden?")), 800)

        val snapshot = stack.snapshot(900)
        assertEquals("Neden?", snapshot.anchor?.text)
        val line = snapshot.lines.single()
        assertEquals("Gitmemiz gerek.", line.text)
        assertEquals(1, line.level)
        assertTrue(line.alpha < 1f && line.scale < 1f)
    }

    @Test
    fun `uc altyazi en cok uc segment gosterir ve eskisi daha soluktur`() {
        val stack = SubtitleStack()
        stack.update(listOf(subtitle("Bir.")), 0)
        stack.update(listOf(subtitle("İki.")), 500)
        stack.update(listOf(subtitle("Üç.")), 1_000)

        val lines = stack.snapshot(1_100).lines
        assertEquals(listOf("İki.", "Bir."), lines.map { it.text })
        assertEquals(listOf(1, 2), lines.map { it.level })
        assertTrue(lines[1].alpha < lines[0].alpha && lines[1].scale < lines[0].scale)
    }

    @Test
    fun `dort ve daha fazla altyazida en eski atilir`() {
        val stack = SubtitleStack()
        listOf("Bir.", "İki.", "Üç.", "Dört.", "Beş.").forEachIndexed { i, text -> stack.update(listOf(subtitle(text)), i * 400L) }

        assertEquals(listOf("Dört.", "Üç."), stack.texts(1_700))
    }

    @Test
    fun `okunacak kadar kalan satir yigina girmez ve sureklilik kopar`() {
        val stack = SubtitleStack()
        stack.update(listOf(subtitle("Bir.")), 0)
        stack.update(listOf(subtitle("İki.")), 500)
        // "İki." okunma süresinden (1 sn + 4 harf) uzun ekranda kaldı.
        stack.update(listOf(subtitle("Üç.")), 500 + SubtitleStack.readingMillis("İki.") + 100)

        assertTrue(stack.snapshot(3_000).lines.isEmpty())
    }

    @Test
    fun `uzun altyazinin okuma suresi uzar ama sinirlidir`() {
        val long = "Bu çok uzun bir altyazı satırı, oyuncunun okuması biraz zaman alır ve iki satıra bölünür."
        assertTrue(SubtitleStack.readingMillis(long) > SubtitleStack.readingMillis("Evet."))
        assertEquals(5_000L, SubtitleStack.readingMillis(long.repeat(5)))

        val stack = SubtitleStack()
        stack.update(listOf(subtitle(long)), 0)
        stack.update(listOf(subtitle("Peki.")), 3_000)
        assertEquals(listOf(long), stack.texts(3_100))
    }

    @Test
    fun `onceki satir sure dolunca solar ve kalkar`() {
        val stack = SubtitleStack(holdMillis = 4_000, fadeMillis = 600)
        stack.update(listOf(subtitle("Bir.")), 0)
        stack.update(listOf(subtitle("İki.")), 500)

        assertEquals(4_000L, stack.nextChangeIn(500))
        assertEquals(1f * SubtitleStack.alphaFor(1), stack.snapshot(4_400).lines.single().alpha)
        val fading = stack.snapshot(4_800).lines.single().alpha
        assertTrue(fading < SubtitleStack.alphaFor(1) && fading > 0f)
        assertEquals(0L, stack.nextChangeIn(4_800))
        assertTrue(stack.snapshot(5_100).lines.isEmpty())
        assertNull(stack.nextChangeIn(5_100))
    }

    @Test
    fun `ayni satirin uzamasi ya da yeniden okunmasi yigina girmez`() {
        val stack = SubtitleStack()
        stack.update(listOf(subtitle("Gitmemiz")), 0)
        stack.update(listOf(subtitle("Gitmemiz gerek.")), 300)
        stack.update(listOf(subtitle("gitmemiz  gerek")), 600)

        assertTrue(stack.snapshot(700).lines.isEmpty())
        // Sonraki gerçek yeni satırda önceki, son okunan hâliyle yığına girer.
        stack.update(listOf(subtitle("Neden?")), 900)
        assertEquals(listOf("gitmemiz  gerek"), stack.texts(1_000))
    }

    @Test
    fun `altyazi bir an kaybolup ayni satir donerse yigin bozulmaz`() {
        val stack = SubtitleStack()
        stack.update(listOf(subtitle("Bir.")), 0)
        stack.update(listOf(subtitle("İki.")), 500)
        stack.update(emptyList(), 700)
        stack.update(listOf(subtitle("İki.")), 900)

        assertEquals(listOf("Bir."), stack.texts(1_000))
    }

    @Test
    fun `kisa bosluktan sonra gelen yeni satir oncekini ustte tutar`() {
        val stack = SubtitleStack(sceneGapMillis = 1_500)
        stack.update(listOf(subtitle("Bir.")), 0)
        stack.update(emptyList(), 600)
        stack.update(listOf(subtitle("İki.")), 1_000)

        assertEquals(listOf("Bir."), stack.texts(1_100))
    }

    @Test
    fun `yeni sahnede eski satirlar gosterilmez`() {
        val stack = SubtitleStack(sceneGapMillis = 1_500)
        stack.update(listOf(subtitle("Bir.")), 0)
        stack.update(listOf(subtitle("İki.")), 500)
        stack.update(emptyList(), 800)

        // Altyazı kalkınca önceki satırlar kısa süre son konumda kalır, sonra temizlenir.
        assertEquals(listOf("Bir."), stack.texts(1_000))
        assertTrue(stack.snapshot(2_400).lines.isEmpty())
        // Yeni sahnenin ilk satırının üstüne eski satır çıkmaz.
        stack.update(listOf(subtitle("Üç.")), 2_500)
        assertTrue(stack.snapshot(2_600).lines.isEmpty())
    }

    @Test
    fun `duraklatip surdurunce eski satirlar geri gelmez`() {
        val stack = SubtitleStack()
        stack.update(listOf(subtitle("Bir.")), 0)
        stack.update(listOf(subtitle("İki.")), 500)
        // Masaüstü durdurulunca boş liste gönderir; sürdürünce aynı replik yeniden gelir.
        stack.update(emptyList(), 600)
        stack.update(emptyList(), 10_000)
        stack.update(listOf(subtitle("İki.")), 20_000)

        assertTrue(stack.snapshot(20_100).lines.isEmpty())
    }

    @Test
    fun `uykudan uyaninca saat ilerlemis oldugundan bayat satir gosterilmez`() {
        val stack = SubtitleStack()
        stack.update(listOf(subtitle("Bir.")), 0)
        stack.update(listOf(subtitle("İki.")), 500)
        // elapsedRealtime uyku süresini de sayar: uyanınca satırların süresi dolmuştur.
        assertTrue(stack.snapshot(60 * 60_000L).lines.isEmpty())
        stack.update(listOf(subtitle("Üç.")), 60 * 60_000L)
        assertTrue(stack.snapshot(60 * 60_000L + 100).lines.isEmpty())
    }

    @Test
    fun `yeniden baglanmada sifirlanir`() {
        val stack = SubtitleStack()
        stack.update(listOf(subtitle("Bir.")), 0)
        stack.update(listOf(subtitle("İki.")), 500)
        stack.reset()

        assertSame(StackSnapshot.EMPTY, stack.snapshot(600))
        stack.update(listOf(subtitle("Üç.")), 700)
        assertTrue(stack.snapshot(800).lines.isEmpty())
    }

    @Test
    fun `turu olmayan bloklar yigina girmez`() {
        val stack = SubtitleStack()
        // Eski masaüstü sürümü "kind" göndermez: davranış eskisi gibi.
        stack.update(listOf(subtitle("Bir.").copy(kind = null), menu), 0)
        stack.update(listOf(subtitle("İki.").copy(kind = null), menu), 500)
        assertTrue(stack.snapshot(600).lines.isEmpty())

        // Menü değişimi altyazı yığınını etkilemez.
        stack.update(listOf(subtitle("Bir."), menu), 1_000)
        stack.update(listOf(subtitle("İki."), menu.copy(text = "Harita")), 1_500)
        assertEquals(listOf("Bir."), stack.texts(1_600))
    }

    @Test
    fun `capa guncel altyazinin konumudur ve konum degistirilmez`() {
        val stack = SubtitleStack()
        val moved = subtitle("İki.").copy(x = 0.1f, y = 0.6f, w = 0.5f)
        stack.update(listOf(subtitle("Bir.")), 0)
        stack.update(listOf(moved), 500)

        assertEquals(moved, stack.snapshot(600).anchor)
        assertFalse(stack.snapshot(600).lines.isEmpty())
    }

    @Test
    fun `eski zaman damgasiyla bayat satir gosterilmez`() {
        val stack = SubtitleStack()
        stack.update(listOf(subtitle("Bir.")), 10_000)
        stack.update(listOf(subtitle("İki.")), 10_500)

        // Saat geri giderse (yanlış damga) satır gösterilmez, yığın bozulmaz.
        assertTrue(stack.snapshot(9_000).lines.isEmpty())
        assertEquals(listOf("Bir."), stack.texts(10_600))
        // Eski damgalı bir güncelleme yeni satırı yığına sokmaz, kalanlar süresi dolunca temizlenir.
        stack.update(listOf(subtitle("İki.")), 9_000)
        assertEquals(listOf("Bir."), stack.texts(10_600))
        assertTrue(stack.snapshot(20_000).lines.isEmpty())
    }

    @Test
    fun `ayni satir karsilastirmasi`() {
        assertTrue(SubtitleStack.sameLine("Gitmemiz gerek.", "gitmemiz gerek"))
        assertTrue(SubtitleStack.sameLine("Gitmemiz", "Gitmemiz gerek."))
        assertFalse(SubtitleStack.sameLine("Gitmemiz gerek.", "Neden?"))
        assertFalse(SubtitleStack.sameLine("...", "Neden?"))
    }
}
