package com.isacetin.ps5ceviri.overlay

import com.isacetin.ps5ceviri.network.Block
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class OverlayStateTest {

    private val remote = Block(id = "r1", x = 0.1f, y = 0.8f, w = 0.6f, h = 0.08f, lineH = 0.03f, text = "Kapı kilitli")

    private fun TestScope.store() = OverlayStore(backgroundScope)

    /** [millis] sanal zamanı ilerletir ve o ana kadar zamanı gelen işleri çalıştırır. */
    private fun TestScope.advance(millis: Long) {
        advanceTimeBy(millis)
        runCurrent()
    }

    @Test
    fun `test yazisi demo blogunu ekler`() = runTest {
        val store = store()
        store.showDemo()

        val block = store.blocks.value.single()
        assertEquals("demo-1", block.id)
        assertEquals("KAPI KİLİTLİ", block.text)
        assertEquals(0.25f, block.x)
        assertEquals(0.42f, block.y)
        assertEquals(0.50f, block.w)
        assertEquals(0.10f, block.h)
        assertEquals(0.04f, block.lineH)
    }

    @Test
    fun `test yazisi iki kez gosterilince tek blok kalir`() = runTest {
        val store = store()
        store.showDemo()
        store.showDemo()

        assertEquals(1, store.blocks.value.size)
    }

    @Test
    fun `test yazisini kaldir tum bloklari temizler`() = runTest {
        val store = store()
        store.showDemo()
        store.applyRemote(listOf(remote))

        store.clearAll()

        assertTrue(store.blocks.value.isEmpty())
    }

    @Test
    fun `yeni mesaj eski listenin yerini alir`() = runTest {
        val store = store()
        store.applyRemote(listOf(remote, remote.copy(id = "r2")))
        store.applyRemote(listOf(remote.copy(id = "r3")))

        assertEquals(listOf("r3"), store.blocks.value.map { it.id })
    }

    @Test
    fun `bos blocks listesi overlay'i temizler`() = runTest {
        val store = store()
        store.applyRemote(listOf(remote))

        store.applyRemote(emptyList())

        assertTrue(store.blocks.value.isEmpty())
    }

    @Test
    fun `websocket bloklari 3 saniye sonra temizlenir`() = runTest {
        val store = store()
        store.applyRemote(listOf(remote))

        advance(2_999)
        assertEquals(listOf(remote), store.blocks.value)

        advance(1)
        assertTrue(store.blocks.value.isEmpty())
    }

    @Test
    fun `heartbeat ayni liste olsa da timeout'u sifirlar`() = runTest {
        val store = store()
        store.applyRemote(listOf(remote))

        // 4 saniye boyunca saniyede bir aynı liste: hiç temizlenmemeli.
        repeat(4) {
            advance(1_000)
            store.applyRemote(listOf(remote))
            assertEquals(listOf(remote), store.blocks.value)
        }

        advance(2_999)
        assertEquals(listOf(remote), store.blocks.value)
        advance(1)
        assertTrue(store.blocks.value.isEmpty())
    }

    @Test
    fun `timeout sonrasi yeni mesaj overlay'i tekrar gosterir`() = runTest {
        val store = store()
        store.applyRemote(listOf(remote))
        advance(3_000)
        assertTrue(store.blocks.value.isEmpty())

        store.applyRemote(listOf(remote))

        assertEquals(listOf(remote), store.blocks.value)
    }

    @Test
    fun `demo blogu timeout'tan etkilenmez`() = runTest {
        val store = store()
        store.showDemo()
        store.applyRemote(listOf(remote))
        assertEquals(listOf("demo-1", "r1"), store.blocks.value.map { it.id })

        advance(10_000)

        assertEquals(listOf("demo-1"), store.blocks.value.map { it.id })
    }

    @Test
    fun `bos liste demo blogunu silmez`() = runTest {
        val store = store()
        store.showDemo()

        store.applyRemote(emptyList())

        assertEquals(listOf("demo-1"), store.blocks.value.map { it.id })
    }
}
