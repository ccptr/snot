package app.snot.editor

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.input.pointer.PointerType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.positionChange
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.IntSize
import app.snot.ffi.BlockKind
import app.snot.ffi.PaperArg
import app.snot.ffi.ToolArg
import app.snot.ffi.inkColor
import app.snot.ffi.inkWidth
import app.snot.ffi.paperSpacing
import app.snot.ffi.strokeOutline
import app.snot.ui.Snot
import app.snot.ui.argb
import java.nio.ByteBuffer
import java.nio.ByteOrder

private val INK_WIDTH = inkWidth()
private val PAPER_SPACING = paperSpacing()

/** A closed polygon from packed little-endian `f32` x, y pairs, in ink units. */
private fun outlinePath(bytes: ByteArray): Path {
    val path = Path()
    val buf = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN).asFloatBuffer()
    if (buf.remaining() < 6) return path
    path.moveTo(buf.get(), buf.get())
    while (buf.remaining() >= 2) path.lineTo(buf.get(), buf.get())
    path.close()
    return path
}

/** Stroke input collected as packed little-endian `f32` x, y, pressure. */
private class Points {
    private var buf = ByteBuffer.allocate(12 * 256).order(ByteOrder.LITTLE_ENDIAN)
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
}

/**
 * A drawing block. In a drawing tool every touch draws; while typing only
 * a stylus does, so a finger still scrolls the page.
 */
@Composable
fun InkCanvas(state: EditorState, id: String, revision: Long, ink: BlockKind.Ink, modifier: Modifier = Modifier) {
    val c = Snot.colors
    val density = LocalDensity.current
    var size by remember { mutableStateOf(IntSize.Zero) }
    val scale = if (size.width > 0) size.width / INK_WIDTH else 1f
    val heightDp = with(density) { (ink.height * scale).toDp() }

    val strokes = remember(id, revision, state.dark) {
        state.editor.inkPaths(id, state.dark).map { Triple(outlinePath(it.outline), argb(it.argb), it.tool) }
    }
    var live by remember { mutableStateOf<Pair<Path, Color>?>(null) }
    var eraserAt by remember { mutableStateOf<Offset?>(null) }
    val tool by rememberUpdatedState(state.tool)
    val dark by rememberUpdatedState(state.dark)

    Canvas(
        modifier
            .fillMaxWidth()
            .height(heightDp)
            .onSizeChanged { size = it }
            .pointerInput(id, state.trashed) {
                if (state.trashed) return@pointerInput
                awaitEachGesture {
                    val down = awaitFirstDown(requireUnconsumed = false)
                    val stylus = down.type == PointerType.Stylus || down.type == PointerType.Eraser
                    val t = when {
                        down.type == PointerType.Eraser -> InkTool.ERASER
                        tool == InkTool.TYPE && stylus -> InkTool.PEN
                        else -> tool
                    }
                    if (t == InkTool.TYPE) return@awaitEachGesture
                    val k = INK_WIDTH / size.width.coerceAtLeast(1)
                    // Touch screens report contact size, not force, as pressure.
                    fun pressure(p: Float) = if (stylus) p.coerceIn(0.05f, 1f) else 0.5f
                    down.consume()
                    val points = Points()
                    val pen = state.pens.getValue(t)
                    val strokeTool = t.stroke
                    fun at(pos: Offset, p: Float) {
                        if (strokeTool == null) {
                            eraserAt = pos
                            state.erase(id, pos.x * k, pos.y * k)
                        } else {
                            points.add(pos.x * k, pos.y * k, pressure(p))
                            val alpha = if (strokeTool == ToolArg.HIGHLIGHTER) 0.35f else 1f
                            live = outlinePath(strokeOutline(points.bytes(), strokeTool, pen.width)) to
                                argb(inkColor(pen.color, dark)).copy(alpha = alpha)
                        }
                    }
                    at(down.position, down.pressure)
                    while (true) {
                        val event = awaitPointerEvent()
                        val change = event.changes.firstOrNull { it.id == down.id } ?: break
                        for (h in change.historical) at(h.position, change.pressure)
                        if (change.positionChange() != Offset.Zero) at(change.position, change.pressure)
                        change.consume()
                        if (!change.pressed) break
                    }
                    if (strokeTool != null) {
                        state.addStroke(id, strokeTool, pen, points.bytes())
                    } else {
                        state.endGesture()
                    }
                    live = null
                    eraserAt = null
                }
            },
    ) {
        paper(ink.paper, PAPER_SPACING * scale, c.paperRule)
        scale(scale, pivot = Offset.Zero) {
            for ((path, color, _) in strokes) drawPath(path, color)
            live?.let { (path, color) -> drawPath(path, color) }
        }
        eraserAt?.let {
            drawCircle(c.textMuted, state.pens.getValue(InkTool.ERASER).width * scale, it, style = Stroke(1.5f * density.density))
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
            var y = step * 2
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
            val r = (1.6f * step / PAPER_SPACING).coerceAtLeast(1.2f * density)
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
