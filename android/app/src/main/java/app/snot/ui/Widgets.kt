package app.snot.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

/** A square icon button that shows when its tool or format is on. */
@Composable
fun ToolButton(
    icon: ImageVector,
    label: String,
    modifier: Modifier = Modifier,
    active: Boolean = false,
    enabled: Boolean = true,
    size: Dp = 44.dp,
    onClick: () -> Unit,
) {
    val c = Snot.colors
    val tint = when {
        !enabled -> c.textFaint
        active -> c.accent
        else -> c.text
    }
    Box(
        modifier
            .size(size)
            .clip(RoundedCornerShape(10.dp))
            .background(if (active) c.accentSoft else Color.Transparent)
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick)
            .semantics { contentDescription = label },
        contentAlignment = Alignment.Center,
    ) {
        Icon(icon, contentDescription = null, tint = tint, modifier = Modifier.size(22.dp))
    }
}

/** A round colour choice; [selected] rings it in the accent. */
@Composable
fun Swatch(color: Color, label: String, selected: Boolean, size: Dp = 30.dp, onClick: () -> Unit) {
    val c = Snot.colors
    Box(
        Modifier
            .size(size + 10.dp)
            .clip(CircleShape)
            .clickable(role = Role.Button, onClick = onClick)
            .semantics { contentDescription = label },
        contentAlignment = Alignment.Center,
    ) {
        Box(
            Modifier
                .size(size)
                .then(if (selected) Modifier.border(2.dp, c.accent, CircleShape) else Modifier)
                .clip(CircleShape)
                .background(color)
                .border(1.dp, c.border, CircleShape),
        )
    }
}

private val shortTime = DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT)
private val monthDay = DateTimeFormatter.ofPattern("MMM d")
private val monthDayYear = DateTimeFormatter.ofPattern("MMM d, yyyy")

/** When a note changed, as briefly as is unambiguous. */
fun dateLabel(ms: Long): String {
    val t = Instant.ofEpochMilli(ms).atZone(ZoneId.systemDefault())
    val day = t.toLocalDate()
    val today = LocalDate.now()
    return when {
        day == today -> t.format(shortTime)
        day == today.minusDays(1) -> "Yesterday"
        day.year == today.year -> t.format(monthDay)
        else -> t.format(monthDayYear)
    }
}

/** A colour token's name for people. */
fun colorName(token: String): String = token.replaceFirstChar { it.uppercase() }
