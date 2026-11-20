package com.isacetin.ps5ceviri.overlay

import android.annotation.SuppressLint
import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.RectF
import android.graphics.Typeface
import android.os.SystemClock
import android.text.TextUtils
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

private fun RectF.toPixelRect() = PixelRect(left, top, width(), height())

private fun PixelRect.intersects(other: PixelRect) =
    left < other.left + other.width && other.left < left + width && top < other.top + other.height && other.top < top + height

/** Önceki altyazıların çıkabileceği en üst nokta: ekranın üst güvenli payı ve yığının en çok yüksekliği. */
fun historyTopLimit(anchorTop: Float, screenHeight: Int): Float =
    max(screenHeight * SAFE_TOP_RATIO, anchorTop - screenHeight * MAX_STACK_RATIO)

/**
 * Önceki altyazı kutularının yeri: [anchor] (güncel altyazının kutusu) üstüne, [gap] aralıkla,
 * [above] verilirse onun da üstüne (çevirinin üstündeki orijinal altyazı kapanmasın),
 * yatayda onun ortasına hizalı, alttan üste. [sizes] seviye sırasıyla her kutunun (genişlik,
 * yükseklik) boyutudur. Bir kutu [topLimit]'in üstüne taşıyor ya da [others]'tan (öbür çeviri
 * kutuları: HUD, menü) biriyle çakışıyorsa o ve üstündekiler yerleştirilmez. Kutular ekrandan
 * yatayda taşmaz.
 */
fun placeHistory(
    anchor: PixelRect,
    sizes: List<Pair<Float, Float>>,
    gap: Float,
    topLimit: Float,
    others: List<PixelRect>,
    screenWidth: Int,
    above: Float = anchor.top,
): List<PixelRect> {
    val placed = ArrayList<PixelRect>(sizes.size)
    var bottom = min(anchor.top, above) - gap
    for ((width, height) in sizes) {
        val w = min(width, screenWidth.toFloat())
        val left = (anchor.left + anchor.width / 2f - w / 2f).coerceIn(0f, screenWidth - w)
        val rect = PixelRect(left, bottom - height, w, height)
        if (rect.top < topLimit || others.any { it.intersects(rect) }) break
        placed += rect
        bottom = rect.top - gap
    }
    return placed
}

// Altyazı yığını (önceki satırlar), ekran yüksekliğine oran.
internal const val STACK_GAP_RATIO = 0.006f
internal const val SAFE_TOP_RATIO = 0.05f
internal const val MAX_STACK_RATIO = 0.22f

/**
 * Çeviri bloklarını Canvas üzerine çizen, girdi almayan şeffaf overlay view'ı.
 * Koordinatlar [screenSize]'ın verdiği ekran boyutuna göre hesaplanır; view ekranın
 * (0,0) noktasında değilse çizim konumuna göre kaydırılır.
 *
 * [stack] verilirse güncel altyazının önceki satırları (bkz. [SubtitleStack]) altyazı kutusunun
 * hemen üstüne, daha küçük ve soluk çizilir. Yığın, ekranın üst güvenli payına, ekranın
 * [MAX_STACK_RATIO]'ından fazlasına ya da başka bir çeviri kutusuna taşacaksa o satır çizilmez:
 * HUD ve menüler kapanmaz.
 */
@SuppressLint("ViewConstructor")
class TranslationOverlayView(
    context: Context,
    private val screenSize: () -> Pair<Int, Int>,
    private val stack: SubtitleStack? = null,
    private val clock: () -> Long = SystemClock::elapsedRealtime,
) : View(context) {

    var mode = DisplayMode.POSITION

    private class RenderedBlock(
        val id: String,
        val background: RectF,
        val radius: Float,
        val layout: StaticLayout,
        val textLeft: Float,
        val textTop: Float,
        val fontSize: Float,
    )

    /** Önceki satırın çizim hâli; metin ve boyut değişmedikçe yeniden kurulmaz. */
    private class HistoryLayout(val key: String, val layout: StaticLayout, val paint: TextPaint)

    private var blocks = emptyList<Block>()
    private var rendered = emptyList<RenderedBlock>()
    private val location = IntArray(2)

    private val backgroundPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = Color.argb(225, 0, 0, 0) }
    private val baseTextPaint = TextPaint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.WHITE
        typeface = Typeface.DEFAULT_BOLD
    }
    private val measurePaint = TextPaint(baseTextPaint)
    private val historyBackground = Paint(backgroundPaint)
    private var historyLayouts = emptyMap<String, HistoryLayout>()
    private val redrawStack = Runnable { invalidate() }

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
        val now = clock()
        val snapshot = stack?.snapshot(now) ?: StackSnapshot.EMPTY
        if (rendered.isEmpty() && snapshot.lines.isEmpty()) return
        getLocationOnScreen(location)
        canvas.withTranslation(-location[0].toFloat(), -location[1].toFloat()) {
            for (block in rendered) {
                drawRoundRect(block.background, block.radius, block.radius, backgroundPaint)
                withTranslation(block.textLeft, block.textTop) { block.layout.draw(this) }
            }
            drawHistory(this, snapshot)
        }
        scheduleStackRedraw(now)
    }

    override fun onDetachedFromWindow() {
        removeCallbacks(redrawStack)
        super.onDetachedFromWindow()
    }

    /** Solma sürerken her karede, yoksa bir sonraki değişim anında yeniden çizilir. */
    private fun scheduleStackRedraw(now: Long) {
        removeCallbacks(redrawStack)
        val next = stack?.nextChangeIn(now) ?: return
        if (next <= 0L) postInvalidateOnAnimation() else postDelayed(redrawStack, next)
    }

    private fun drawHistory(canvas: Canvas, snapshot: StackSnapshot) {
        val anchorBlock = snapshot.anchor ?: return
        if (snapshot.lines.isEmpty()) return
        val (screenWidth, screenHeight) = screenSize().takeIf { it.first > 0 && it.second > 0 } ?: (width to height)
        if (screenWidth <= 0 || screenHeight <= 0) return
        // Altyazı ekrandaysa onun kutusu; az önce kalktıysa son konumu.
        val anchor = rendered.firstOrNull { it.id == anchorBlock.id } ?: layoutBlock(anchorBlock, screenWidth, screenHeight)
        val padding = screenHeight * PADDING_RATIO
        val gap = screenHeight * STACK_GAP_RATIO
        val textWidth = (anchor.background.width() - 2 * padding).roundToInt().coerceAtLeast(1)
        val used = HashMap<String, HistoryLayout>()
        val lines = snapshot.lines.sortedBy { it.level }
        val layouts = lines.map { line ->
            val fontSize = max(anchor.fontSize * line.scale, screenHeight * MIN_FONT_RATIO * 0.8f)
            val key = "${line.text}|$fontSize|$textWidth"
            val cached = historyLayouts[key] ?: used[key] ?: run {
                val paint = TextPaint(baseTextPaint).apply { textSize = fontSize }
                HistoryLayout(key, buildLayout(line.text, paint, textWidth, maxLines = HISTORY_MAX_LINES), paint)
            }
            used[key] = cached
            cached
        }
        // Çeviri orijinalin altındaysa yığın orijinal altyazının üstünden başlar.
        val above = anchorBlock.srcY?.let { it * screenHeight - padding } ?: anchor.background.top
        val placed = placeHistory(
            anchor = anchor.background.toPixelRect(),
            sizes = layouts.map { widestLine(it.layout) + 2 * padding to it.layout.height + 2 * padding },
            gap = gap,
            topLimit = historyTopLimit(min(anchor.background.top, above), screenHeight),
            others = rendered.filter { it.id != anchor.id }.map { it.background.toPixelRect() },
            screenWidth = screenWidth,
            above = above,
        )
        for ((index, rect) in placed.withIndex()) {
            val line = lines[index]
            val cached = layouts[index]
            val layout = cached.layout
            val background = RectF(rect.left, rect.top, rect.left + rect.width, rect.top + rect.height)
            val alpha = (line.alpha * 255).roundToInt().coerceIn(0, 255)
            historyBackground.alpha = alpha * backgroundPaint.alpha / 255
            cached.paint.alpha = alpha
            cached.paint.setShadowLayer(cached.paint.textSize * 0.12f, 0f, cached.paint.textSize * 0.06f, Color.argb(alpha, 0, 0, 0))
            val radius = min(background.height() * 0.2f, screenHeight * 0.012f)
            canvas.drawRoundRect(background, radius, radius, historyBackground)
            canvas.withTranslation(background.centerX() - layout.width / 2f, background.top + padding) { layout.draw(this) }
        }
        historyLayouts = used
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
        return RenderedBlock(block.id, background, radius, layout, rect.left, textTop, fontSize)
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

    /**
     * Satırlar dengeli bölünür (uzun çeviri üst satırı doldurup alta tek sözcük bırakmaz) ve sözcük
     * ortasından tirelenmez; yalnızca kutudan geniş tek bir sözcük bölünür.
     *
     * `Layout.BREAK_STRATEGY_BALANCED` lint'in istediği `LineBreaker` sabitiyle aynı değerdir (2);
     * `LineBreaker` API 29'da geldiği için minSdk 26'da `Layout`'unki kullanılır.
     */
    @SuppressLint("WrongConstant")
    private fun buildLayout(text: String, paint: TextPaint, width: Int, maxLines: Int = Int.MAX_VALUE): StaticLayout =
        StaticLayout.Builder.obtain(text, 0, text.length, paint, width.coerceAtLeast(1))
            .setAlignment(Layout.Alignment.ALIGN_CENTER)
            .setIncludePad(false)
            .setBreakStrategy(Layout.BREAK_STRATEGY_BALANCED)
            .setHyphenationFrequency(Layout.HYPHENATION_FREQUENCY_NONE)
            .apply { if (maxLines != Int.MAX_VALUE) setMaxLines(maxLines).setEllipsize(TextUtils.TruncateAt.END) }
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

        const val HISTORY_MAX_LINES = 2
    }
}
