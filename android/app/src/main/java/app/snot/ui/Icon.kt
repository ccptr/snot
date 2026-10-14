package app.snot.ui

import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.unit.dp
import androidx.compose.ui.graphics.Color

/**
 * Builds one of the shared line icons: a 24×24 viewport of round-capped
 * 2-unit strokes, with `true` marking the paths that are filled instead.
 * Icons are tinted at the call site, so black here only marks coverage.
 */
internal fun icon(name: String, vararg paths: Pair<String, Boolean>): ImageVector {
    val builder = ImageVector.Builder(
        name = name,
        defaultWidth = 24.dp,
        defaultHeight = 24.dp,
        viewportWidth = 24f,
        viewportHeight = 24f,
    )
    for ((data, filled) in paths) {
        builder.addPath(
            pathData = addPathNodes(data),
            fill = if (filled) SolidColor(Color.Black) else null,
            stroke = if (filled) null else SolidColor(Color.Black),
            strokeLineWidth = 2f,
            strokeLineCap = StrokeCap.Round,
            strokeLineJoin = StrokeJoin.Round,
        )
    }
    return builder.build()
}
