package com.isacetin.ps5ceviri.overlay

/** Belirli bir font boyutunda yerleştirilmiş metnin kapladığı alan (piksel). */
data class TextSize(val width: Float, val height: Float)

/** Android'e bağımlı değil; ölçüm [fit]'e verilen lambda ile yapılır. */
object FontFitter {
    /**
     * Metnin kutuya sığdığı en büyük font boyutunu döner. [initialSize]'dan başlar,
     * sığmadıkça [step] kadar küçültür; [minSize]'ın altına inmez (sığmasa bile).
     *
     * [measure] metni verilen font boyutunda ve en fazla `maxWidth` genişliğinde
     * (gerekirse satır kaydırarak) yerleştirip kapladığı alanı döner.
     */
    fun fit(
        text: String,
        boxWidth: Float,
        boxHeight: Float,
        initialSize: Float,
        minSize: Float,
        maxSize: Float,
        step: Float = 1f,
        measure: (text: String, fontSize: Float, maxWidth: Float) -> TextSize,
    ): Float {
        require(minSize in 0f..maxSize) { "minSize ($minSize) 0..maxSize ($maxSize) aralığında olmalı" }
        require(step > 0f) { "step pozitif olmalı" }

        var size = initialSize.coerceIn(minSize, maxSize)
        while (size > minSize) {
            val measured = measure(text, size, boxWidth)
            if (measured.width <= boxWidth && measured.height <= boxHeight) return size
            size = (size - step).coerceAtLeast(minSize)
        }
        return minSize
    }
}
