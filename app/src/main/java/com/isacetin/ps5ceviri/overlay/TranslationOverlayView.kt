package com.isacetin.ps5ceviri.overlay

import android.annotation.SuppressLint
import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.RectF
import android.graphics.Typeface
import android.text.Layout
import android.text.StaticLayout
import android.text.TextPaint
import android.view.View
import androidx.core.graphics.withTranslation
import com.isacetin.ps5ceviri.network.Block
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

data class PixelRect(val left: Float, val top: Float, val width: Float, val height: Float)

/** Normalize (0..1, origin sol üst) koordinatı ekran pikseline çevirir. */
fun Block.toPixelRect(screenWidth: Int, screenHeight: Int) = PixelRect(
    left = x * screenWidth,
    top = y * screenHeight,
    width = w * screenWidth,
    height = h * screenHeight,
)

/**
 * Kutuyu [targetWidth] genişliğine, merkezi sabit kalacak şekilde yatayda genişletir; ekrandan
 * taşmaması için kaydırılır. Türkçe çeviri İngilizceden uzun olduğunda kullanılır.
 */
fun PixelRect.widenedTo(targetWidth: Float, screenWidth: Int): PixelRect {
    val screen = screenWidth.toFloat()
    val width = targetWidth.coerceIn(min(this.width, screen), screen)
    val centered = left + this.width / 2f - width / 2f
    return copy(left = centered.coerceIn(0f, screenWidth - width), width = width)
}

/**
 * Çeviri bloklarını Canvas üzerine çizen, girdi almayan şeffaf overlay view'ı.
 * Koordinatlar [screenSize]'ın verdiği ekran boyutuna göre hesaplanır; view ekranın
 * (0,0) noktasında değilse çizim konumuna göre kaydırılır.
 */
@SuppressLint("ViewConstructor")
class TranslationOverlayView(
    context: Context,
    private val screenSize: () -> Pair<Int, Int>,
) : View(context) {

    var mode = DisplayMode.POSITION

    private class RenderedBlock(val background: RectF, val radius: Float, val layout: StaticLayout, val textLeft: Float, val textTop: Float)

    private var blocks = emptyList<Block>()
    private var rendered = emptyList<RenderedBlock>()
    private val location = IntArray(2)

    private val backgroundPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = Color.argb(225, 0, 0, 0) }
    private val baseTextPaint = TextPaint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.WHITE
        typeface = Typeface.DEFAULT_BOLD
    }
    private val measurePaint = TextPaint(baseTextPaint)

    init {
        isFocusable = false
        isClickable = false
        importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
    }

    fun setBlocks(newBlocks: List<Block>) {
        blocks = newBlocks
        rebuild()
        invalidate()
    }

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        rebuild()
    }

    override fun onDraw(canvas: Canvas) {
        if (rendered.isEmpty()) return
        getLocationOnScreen(location)
        canvas.withTranslation(-location[0].toFloat(), -location[1].toFloat()) {
            for (block in rendered) {
                drawRoundRect(block.background, block.radius, block.radius, backgroundPaint)
                withTranslation(block.textLeft, block.textTop) { block.layout.draw(this) }
            }
        }
    }

    private fun rebuild() {
        val (screenWidth, screenHeight) = screenSize().takeIf { it.first > 0 && it.second > 0 } ?: (width to height)
        rendered = if (screenWidth > 0 && screenHeight > 0) {
            when (mode) {
                // SUBTITLE henüz yok; gelene kadar bloklar kendi konumlarında çizilir.
                DisplayMode.POSITION, DisplayMode.SUBTITLE -> blocks.map { layoutBlock(it, screenWidth, screenHeight) }
            }
        } else {
            emptyList()
        }
    }

    private fun layoutBlock(block: Block, screenWidth: Int, screenHeight: Int): RenderedBlock {
        var rect = block.toPixelRect(screenWidth, screenHeight)
        val minSize = screenHeight * MIN_FONT_RATIO
        // OCR satır kutusu harf yüksekliğine yakındır; font biraz daha büyük başlar.
        val initialSize = if (block.lineH > 0f) block.lineH * screenHeight * LINE_TO_FONT else rect.height * 0.6f

        var fontSize = fitFont(block.text, rect, initialSize, minSize, screenHeight)
        // Minimum fontta bile sığmıyorsa (Türkçe çeviri uzun) yazıyı küçültmek yerine kutuyu genişlet.
        if (!fits(block.text, rect, fontSize)) {
            measurePaint.textSize = fontSize
            val singleLine = measurePaint.measureText(block.text) + fontSize
            rect = rect.widenedTo(min(singleLine, screenWidth * MAX_WIDTH_RATIO), screenWidth)
            fontSize = fitFont(block.text, rect, initialSize, minSize, screenHeight)
        }

        // StaticLayout çizerken paint'i referansla kullanır; her blok kendi paint'ine sahip olmalı.
        val paint = TextPaint(baseTextPaint).apply {
            textSize = fontSize
            setShadowLayer(fontSize * 0.12f, 0f, fontSize * 0.06f, Color.BLACK)
        }
        val layout = buildLayout(block.text, paint, rect.width.roundToInt())
        val textTop = rect.top + (rect.height - layout.height) / 2f

        val padding = screenHeight * PADDING_RATIO
        val background = RectF(
            rect.left - padding,
            min(rect.top, textTop) - padding,
            rect.left + rect.width + padding,
            max(rect.top + rect.height, textTop + layout.height) + padding,
        )
        val radius = min(background.height() * 0.2f, screenHeight * 0.012f)
        return RenderedBlock(background, radius, layout, rect.left, textTop)
    }

    private fun fitFont(text: String, rect: PixelRect, initialSize: Float, minSize: Float, screenHeight: Int): Float =
        FontFitter.fit(
            text = text,
            boxWidth = rect.width.roundToInt().coerceAtLeast(1).toFloat(),
            boxHeight = rect.height,
            initialSize = initialSize,
            minSize = minSize,
            maxSize = screenHeight * MAX_FONT_RATIO,
        ) { candidate, size, maxWidth ->
            measurePaint.textSize = size
            val layout = buildLayout(candidate, measurePaint, maxWidth.toInt())
            TextSize(widestLine(layout), layout.height.toFloat())
        }

    private fun fits(text: String, rect: PixelRect, fontSize: Float): Boolean {
        measurePaint.textSize = fontSize
        val layout = buildLayout(text, measurePaint, rect.width.roundToInt())
        return layout.height <= rect.height * 1.05f && widestLine(layout) <= rect.width
    }

    private fun buildLayout(text: String, paint: TextPaint, width: Int): StaticLayout =
        StaticLayout.Builder.obtain(text, 0, text.length, paint, width.coerceAtLeast(1))
            .setAlignment(Layout.Alignment.ALIGN_CENTER)
            .setIncludePad(false)
            .build()

    private fun widestLine(layout: StaticLayout): Float =
        (0 until layout.lineCount).maxOfOrNull { layout.getLineWidth(it) } ?: 0f

    private companion object {
        // 1080p'de ~30 px: TV'ye birkaç metreden bakarken okunabilir en küçük boyut.
        const val MIN_FONT_RATIO = 0.028f
        const val MAX_FONT_RATIO = 0.12f
        const val LINE_TO_FONT = 1.15f
        const val MAX_WIDTH_RATIO = 0.9f
        const val PADDING_RATIO = 0.01f
    }
}
