package com.isacetin.ps5ceviri.network

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SeqGateTest {

    @Test
    fun `artan seq kabul edilir`() {
        val gate = SeqGate()

        assertTrue(gate.accept(1))
        assertTrue(gate.accept(2))
        assertTrue(gate.accept(10))
    }

    @Test
    fun `eski seq'li mesaj yok sayilir`() {
        val gate = SeqGate()
        assertTrue(gate.accept(123))

        assertFalse(gate.accept(122))
        assertFalse(gate.accept(1))
    }

    @Test
    fun `yok sayilan mesaj son kabul edileni degistirmez`() {
        val gate = SeqGate()
        gate.accept(50)
        gate.accept(10)

        assertFalse(gate.accept(49))
        assertTrue(gate.accept(51))
    }

    @Test
    fun `ayni seq kabul edilir`() {
        val gate = SeqGate()
        gate.accept(7)

        assertTrue(gate.accept(7))
    }

    @Test
    fun `yeni baglanti sayaci sifirlar`() {
        val first = SeqGate()
        first.accept(500)

        // Her yeni WebSocket bağlantısı yeni bir SeqGate alır.
        val second = SeqGate()

        assertTrue(second.accept(1))
    }
}
