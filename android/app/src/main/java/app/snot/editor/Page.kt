package app.snot.editor

import android.graphics.Canvas
import android.graphics.Matrix
import android.graphics.Paint
import android.graphics.PixelFormat
import android.graphics.PorterDuff
import android.graphics.RectF
import android.os.Build
import android.view.SurfaceView
import androidx.annotation.RequiresApi
import androidx.compose.foundation.ScrollState
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.PointerType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.LayoutCoordinates
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.TextUnit
import androidx.graphics.lowlatency.CanvasFrontBufferedRenderer
import app.snot.ffi.InkPath
import app.snot.ffi.PaperArg
import app.snot.ffi.StyleArg
import app.snot.ffi.ToolArg
import app.snot.ffi.inkColor
import app.snot.ffi.pageMetrics
import app.snot.ffi.strokeOutline
import app.snot.ffi.textMetrics
import app.snot.ui.Snot
import app.snot.ui.argb
import java.nio.ByteBuffer
import java.nio.ByteOrder
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch

/** The page's fixed geometry, in page units. */
val PAGE = pageMetrics()

/** How each paragraph style sets its text, in page units. */
val TEXT = StyleArg.entries.associateWith { textMetrics(it) }

/**
 * Page units at the width the page is shown. Everything on the page is
 * sized through this, text included, so the page zooms rather than
 * reflows and handwriting stays over the words it was written on.
 */
@Stable
class PageScale(val px: Float, private val density: Density) {
    fun dp(units: Float): Dp = with(density) { (units * px).toDp() }

    /** Font size; set in pixels so the system font scale does not move text out from under the ink. */
    fun sp(units: Float): TextUnit = with(density) { (units * px).toSp() }
}

val LocalPage = staticCompositionLocalOf<PageScale> { error("no page") }

/** Reads packed little-endian `f32` x, y pairs, in page units. */
private inline fun outline(bytes: ByteArray, moveTo: (Float, Float) -> Unit, lineTo: (Float, Float) -> Unit): Boolean {
    val buf = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN).asFloatBuffer()
    if (buf.remaining() < 6) return false
    moveTo(buf.get(), buf.get())
    while (buf.remaining() >= 2) lineTo(buf.get(), buf.get())
    return true
}

private fun composePath(bytes: ByteArray): Path {
    val path = Path()
    if (outline(bytes, path::moveTo, path::lineTo)) path.close()
    return path
}

private fun platformPath(bytes: ByteArray): android.graphics.Path {
    val path = android.graphics.Path()
    if (outline(bytes, path::moveTo, path::lineTo)) path.close()
    return path
}

/** A finished stroke, outlined once and kept for every frame after. */
class StrokePath(p: InkPath) {
    val path = composePath(p.outline)
    val color = argb(p.argb)
    val highlighter = p.tool == ToolArg.HIGHLIGHTER
    val bottom = path.getBounds().bottom
}

/** Stroke input collected as packed little-endian `f32` x, y, pressure. */
private class Points {
    private var buf = ByteBuffer.allocate(12 * 256).order(ByteOrder.LITTLE_ENDIAN)
    val size get() = buf.position() / 12
    fun add(x: Float, y: Float, pressure: Float) {
        if (buf.remaining() < 12) {
            val bigger = ByteBuffer.allocate(buf.capacity() * 2).order(ByteOrder.LITTLE_ENDIAN)
            buf.flip()
            bigger.put(buf)
            buf = bigger
        }
        buf.putFloat(x).putFloat(y).putFloat(pressure)
    }
    fun bytes(): ByteArray = buf.array().copyOf(buf.position())

    /** Bounds of the last [n] points, in page units. */
    fun tailBounds(n: Int): RectF {
        val r = RectF(Float.MAX_VALUE, Float.MAX_VALUE, -Float.MAX_VALUE, -Float.MAX_VALUE)
        for (i in maxOf(0, size - n) until size) {
            val x = buf.getFloat(i * 12)
            val y = buf.getFloat(i * 12 + 4)
            r.union(x, y)
        }
        return r
    }
}

/** One frame of the stroke being written, for the render thread. */
private class Frame(
    val points: ByteArray,
    val tool: ToolArg,
    val width: Float,
    val argb: Int,
    /** Page units to surface pixels. */
    val matrix: Matrix,
    /** The surface pixels this frame can change. */
    val damage: RectF,
    /** The first stroke since the front buffer was last handed over. */
    val fresh: Boolean,
    /** The pen lifted; the stroke is finished. */
    val done: Boolean,
)

/**
 * The stroke being written, drawn straight to a front buffer over the page.
 * The normal path through Compose and the system compositor costs a few
 * frames between the nib moving and the ink following it; a front buffer
 * is on screen as it is drawn, so ink follows the nib as fast as the
 * display can show it. Each frame redraws only the region the newest
 * points can change, clipped, so what is already on screen never flickers.
 *
 * Finished strokes stay here until writing pauses, then the page takes
 * them over and this layer is hidden.
 */
@RequiresApi(Build.VERSION_CODES.Q)
class FrontInk(view: SurfaceView) {
    /** Where the surface sits, to place page coordinates on it. */
    var coords: LayoutCoordinates? = null
    private val renderer = CanvasFrontBufferedRenderer(view, Painter())
    /** Whether the front buffer holds anything the page might not show yet. */
    private var holding = false
    private var handover: Job? = null

    init {
        view.setZOrderOnTop(true)
        view.holder.setFormat(PixelFormat.TRANSLUCENT)
    }

    fun release() = renderer.release(true)

    /** A stroke starts: whatever was pending stays in the front buffer. */
    fun begin(): Boolean {
        handover?.cancel()
        handover = null
        val fresh = !holding
        holding = true
        return fresh
    }

    fun draw(
        points: ByteArray, tool: ToolArg, width: Float, argb: Int, matrix: Matrix,
        damage: RectF, fresh: Boolean, done: Boolean,
    ) {
        renderer.renderFrontBufferedLayer(Frame(points, tool, width, argb, matrix, damage, fresh, done))
    }

    /**
     * After a short pause in writing, lets the page draw the strokes held
     * here, and hides this layer once the page's frame is on screen.
     */
    fun handOver(state: EditorState, scope: CoroutineScope) {
        handover?.cancel()
        handover = scope.launch {
            // A pause between strokes of one word should not cost a handover.
            repeat(6) { withFrameNanos {} }
            state.frontTail = 0
            // The page draws them in the next frame; give that frame time to
            // reach the display before this layer goes.
            repeat(3) { withFrameNanos {} }
            renderer.cancel()
            holding = false
        }
    }

    /** Hides the layer now, for a scroll or an eraser, which need the page as it is. */
    fun dropNow(state: EditorState) {
        handover?.cancel()
        handover = null
        if (!holding) return
        state.frontTail = 0
        renderer.cancel()
        holding = false
    }

    private class Painter : CanvasFrontBufferedRenderer.Callback<Frame> {
        /** Finished strokes still shown here, in surface pixels. */
        private val held = ArrayList<Pair<android.graphics.Path, Paint>>()

        override fun onDrawFrontBufferedLayer(canvas: Canvas, bufferWidth: Int, bufferHeight: Int, param: Frame) {
            if (param.fresh && param.points.size <= 12) {
                held.clear()
                canvas.drawColor(0, PorterDuff.Mode.CLEAR)
            }
            val live = platformPath(strokeOutline(param.points, param.tool, param.width))
            live.transform(param.matrix)
            val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = param.argb }
            val highlighter = param.tool == ToolArg.HIGHLIGHTER
            canvas.save()
            canvas.clipRect(param.damage)
            canvas.drawColor(0, PorterDuff.Mode.CLEAR)
            // Highlighter goes under everything else, as on the page.
            for ((path, p) in held) if (p.alpha < 255) canvas.drawPath(path, p)
            if (highlighter) canvas.drawPath(live, paint)
            for ((path, p) in held) if (p.alpha == 255) canvas.drawPath(path, p)
            if (!highlighter) canvas.drawPath(live, paint)
            canvas.restore()
            if (param.done) held += live to paint
        }

        override fun onDrawMultiBufferedLayer(
            canvas: Canvas, bufferWidth: Int, bufferHeight: Int, params: Collection<Frame>,
        ) {
            // Never committed to: finished strokes go to the page instead.
        }
    }
}

/** A surface for [FrontInk], under the page in layout but shown above it. */
fun frontInkSurface(context: android.content.Context, onReady: (FrontInk?) -> Unit): SurfaceView {
    val view = SurfaceView(context)
    onReady(if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) FrontInk(view) else null)
    return view
}

/**
 * The page under the text and the handwriting over it, plus the input that
 * writes it. A stylus always writes; a finger writes only with a drawing
 * tool picked, and otherwise reaches the text underneath. Two fingers
 * scroll while a drawing tool is picked, since one finger is drawing.
 */
@Composable
fun Modifier.pageInk(state: EditorState, scale: PageScale, scroll: ScrollState, front: FrontInk?): Modifier {
    val c = Snot.colors
    val view = LocalView.current
    val scope = rememberCoroutineScope()
    // Only where there is no front buffer: the stroke drawn by the page.
    var live by remember { mutableStateOf<Pair<Path, Color>?>(null) }
    var eraserAt by remember { mutableStateOf<Offset?>(null) }
    var pageCoords by remember { mutableStateOf<LayoutCoordinates?>(null) }
    val tool by rememberUpdatedState(state.tool)
    val dark by rememberUpdatedState(state.dark)
    val k = scale.px

    return this
        .onGloballyPositioned { pageCoords = it }
        .drawBehind { paper(state.paper, PAGE.paperSpacing * k, c.paperRule) }
        .pointerInput(state.trashed, k, front) {
            if (state.trashed) return@pointerInput
            awaitEachGesture {
                val down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
                val stylus = down.type == PointerType.Stylus || down.type == PointerType.Eraser
                val t = when {
                    down.type == PointerType.Eraser -> InkTool.ERASER
                    tool == InkTool.TYPE && stylus -> InkTool.PEN
                    else -> tool
                }
                if (t == InkTool.TYPE) return@awaitEachGesture
                // Ahead of the text fields and the scroller, so neither sees it.
                down.consume()
                // Every sample as it comes, not batched up to the next frame.
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                    currentEvent.motionEvent?.let { view.requestUnbufferedDispatch(it) }
                }
                // Touch screens report contact size, not force, as pressure.
                fun pressure(p: Float) = if (stylus) p.coerceIn(0.05f, 1f) else 0.5f
                val points = Points()
                val pen = state.pens.getValue(t)
                val strokeTool = t.stroke
                val color = argb(inkColor(pen.color, dark)).let {
                    if (strokeTool == ToolArg.HIGHLIGHTER) it.copy(alpha = 0.35f) else it
                }
                // Page units to the front buffer's pixels; the page cannot
                // scroll under a stroke, so this holds for all of it.
                val surface = front?.coords?.takeIf { strokeTool != null }
                val origin = pageCoords?.let { surface?.localPositionOf(it, Offset.Zero) }
                val toSurface = origin?.let { Matrix().apply { setScale(k, k); postTranslate(it.x, it.y) } }
                val buffered = front != null && toSurface != null
                val fresh = if (buffered) front!!.begin() else false
                if (strokeTool == null) front?.dropNow(state)
                val reach = pen.width * 0.6f + 1f
                var count = 0

                fun frame(done: Boolean) {
                    val m = toSurface ?: return
                    // The newest points move the most: smoothing and the
                    // tapered tip reach back a few samples.
                    val damage = points.tailBounds(if (count < 12) count else 8)
                    damage.inset(-reach, -reach)
                    m.mapRect(damage)
                    damage.inset(-2f, -2f)
                    front!!.draw(points.bytes(), strokeTool!!, pen.width, color.toArgb(), m, damage, fresh, done)
                }

                fun at(pos: Offset, p: Float) {
                    if (strokeTool == null) {
                        eraserAt = pos
                        state.erase(pos.x / k, pos.y / k)
                        return
                    }
                    points.add(pos.x / k, pos.y / k, pressure(p))
                    count++
                }

                fun show() {
                    if (strokeTool == null) return
                    if (buffered) {
                        frame(done = false)
                    } else {
                        live = composePath(strokeOutline(points.bytes(), strokeTool, pen.width)) to color
                    }
                }

                at(down.position, down.pressure)
                show()
                var panned = false
                while (true) {
                    val event = awaitPointerEvent(PointerEventPass.Initial)
                    if (!stylus && event.changes.count { it.pressed } > 1) {
                        panned = true
                        break
                    }
                    val change = event.changes.firstOrNull { it.id == down.id } ?: break
                    for (h in change.historical) at(h.position, change.pressure)
                    if (change.position != change.previousPosition) at(change.position, change.pressure)
                    show()
                    change.consume()
                    if (!change.pressed) break
                }
                live = null
                eraserAt = null
                if (panned) {
                    // A second finger came down: it was a scroll, not a stroke.
                    front?.dropNow(state)
                    state.endGesture()
                    var last: Float? = null
                    while (true) {
                        val event = awaitPointerEvent(PointerEventPass.Initial)
                        val pressed = event.changes.filter { it.pressed }
                        if (pressed.isEmpty()) break
                        // On screen, not on the page, which moves as it scrolls.
                        val y = pressed.map { it.position.y }.average().toFloat() - scroll.value
                        last?.let { scroll.dispatchRawDelta(it - y) }
                        last = y
                        event.changes.forEach { it.consume() }
                    }
                } else if (strokeTool != null) {
                    if (buffered) frame(done = true)
                    state.addStroke(strokeTool, pen, points.bytes(), front = buffered)
                    if (buffered) front!!.handOver(state, scope)
                } else {
                    state.endGesture()
                }
            }
        }
        .drawWithContent {
            drawContent()
            val shown = state.strokes.size - state.frontTail
            scale(k, pivot = Offset.Zero) {
                // Highlighter goes under everything else.
                for (i in 0 until shown) state.strokes[i].let { if (it.highlighter) drawPath(it.path, it.color) }
                for (i in 0 until shown) state.strokes[i].let { if (!it.highlighter) drawPath(it.path, it.color) }
                live?.let { (path, color) -> drawPath(path, color) }
            }
            eraserAt?.let {
                drawCircle(c.textMuted, state.pens.getValue(InkTool.ERASER).width * k, it, style = Stroke(1.5f * density))
            }
        }
}

private fun DrawScope.paper(paper: PaperArg, step: Float, rule: Color) {
    val w = size.width
    val h = size.height
    val line = 1f.coerceAtLeast(density * 0.75f)
    when (paper) {
        PaperArg.PLAIN -> {}
        PaperArg.LINED -> {
            var y = step
            while (y < h) {
                drawLine(rule, Offset(0f, y), Offset(w, y), line)
                y += step
            }
        }
        PaperArg.GRID -> {
            var y = step
            while (y < h) {
                drawLine(rule, Offset(0f, y), Offset(w, y), line)
                y += step
            }
            var x = step
            while (x < w) {
                drawLine(rule, Offset(x, 0f), Offset(x, h), line)
                x += step
            }
        }
        PaperArg.DOTTED -> {
            val dot = rule.copy(alpha = (rule.alpha * 2.5f).coerceAtMost(1f))
            val r = (1.6f * step / PAGE.paperSpacing).coerceAtLeast(1.2f * density)
            var y = step
            while (y < h) {
                var x = step
                while (x < w) {
                    drawCircle(dot, r, Offset(x, y))
                    x += step
                }
                y += step
            }
        }
    }
}
