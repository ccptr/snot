package app.snot.editor

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withStyle
import app.snot.ffi.RunView
import app.snot.ffi.highlightColor
import app.snot.ffi.inkColor
import app.snot.ui.argb

// The core counts text in Unicode scalar values; Compose counts UTF-16
// units. These convert at the boundary, clamping out-of-range offsets.

/** Code point offset of UTF-16 offset [utf16]. */
fun String.cpOf(utf16: Int): Int = codePointCount(0, utf16.coerceIn(0, length))

/** UTF-16 offset of code point offset [cp]. */
fun String.utf16Of(cp: Int): Int {
    if (cp <= 0) return 0
    return try {
        offsetByCodePoints(0, cp)
    } catch (_: IndexOutOfBoundsException) {
        length
    }
}

/** A text block's styled runs as an [AnnotatedString]. */
fun styled(text: String, runs: List<RunView>, dark: Boolean): AnnotatedString =
    buildAnnotatedString {
        append(text)
        for (r in runs) {
            val start = text.utf16Of(r.start)
            val end = text.utf16Of(r.end)
            if (start >= end) continue
            val decorations = buildList {
                if (r.underline) add(TextDecoration.Underline)
                if (r.strike) add(TextDecoration.LineThrough)
            }
            val style = SpanStyle(
                fontWeight = if (r.bold) FontWeight.Bold else null,
                fontStyle = if (r.italic) FontStyle.Italic else null,
                textDecoration = if (decorations.isEmpty()) null else TextDecoration.combine(decorations),
                color = r.color?.let { argb(inkColor(it, dark)) } ?: Color.Unspecified,
                background = r.highlight?.let { argb(highlightColor(it, dark)) } ?: Color.Unspecified,
            )
            if (style != SpanStyle()) addStyle(style, start, end)
        }
    }

/** A search snippet with its hits (wrapped in U+0002 … U+0003) marked. */
fun snippet(raw: String, hit: SpanStyle): AnnotatedString = buildAnnotatedString {
    var inHit = false
    var from = 0
    fun flush(to: Int) {
        if (to <= from) return
        if (inHit) withStyle(hit) { append(raw, from, to) } else append(raw, from, to)
    }
    for ((i, c) in raw.withIndex()) {
        if (c == '\u0002' || c == '\u0003') {
            flush(i)
            inHit = c == '\u0002'
            from = i + 1
        }
    }
    flush(raw.length)
}

/**
 * The single edit that turns [old] into [new]: the replaced UTF-16 range of
 * [old] and the text that replaces it. Never splits a surrogate pair.
 */
fun diff(old: String, new: String): Triple<Int, Int, String> {
    val max = minOf(old.length, new.length)
    var p = 0
    while (p < max && old[p] == new[p]) p++
    if (p > 0 && Character.isHighSurrogate(old[p - 1])) p--
    var s = 0
    while (s < max - p && old[old.length - 1 - s] == new[new.length - 1 - s]) s++
    if (s > 0 && Character.isLowSurrogate(old[old.length - s])) s--
    return Triple(p, old.length - s, new.substring(p, new.length - s))
}
