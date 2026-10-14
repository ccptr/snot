package app.snot.editor

import android.graphics.BitmapFactory
import android.net.Uri
import android.provider.OpenableColumns
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.VerticalDivider
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.unit.sp
import app.snot.ffi.BlockKind
import app.snot.ffi.BlockView
import app.snot.ffi.FolderInfo
import app.snot.ffi.MarkArg
import app.snot.ffi.PaperArg
import app.snot.ffi.StyleArg
import app.snot.ffi.highlightColor
import app.snot.ffi.highlightColors
import app.snot.ffi.inkColor
import app.snot.ffi.inkColors
import app.snot.ffi.noteBackground
import app.snot.ffi.noteColors
import app.snot.ui.Snot
import app.snot.ui.SnotIcons
import app.snot.ui.Swatch
import app.snot.ui.ToolButton
import app.snot.ui.argb
import app.snot.ui.colorName
import app.snot.ui.dateLabel
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext

/** What the note screen asks of the rest of the app. */
interface EditorHost {
    fun folders(): List<FolderInfo>
    fun close()
    /** The note moved into or out of the trash; show it afresh. */
    fun reopen()
    fun share(title: String, markdown: String)
}

private enum class Sheet { NONE, COLOR, MOVE, DELETE }

@Composable
fun EditorScreen(state: EditorState, host: EditorHost) {
    val c = Snot.colors
    val dark = isSystemInDarkTheme()
    LaunchedEffect(dark) { state.updateDark(dark) }
    // Save a moment after the last change.
    LaunchedEffect(state.version) {
        if (state.version > 0) {
            delay(600)
            state.save()
        }
    }

    val bg = argb(noteBackground(state.info?.color, dark))
    val keyboard = LocalSoftwareKeyboardController.current
    val focus = LocalFocusManager.current
    var sheet by remember { mutableStateOf(Sheet.NONE) }

    Column(
        Modifier
            .fillMaxSize()
            .background(bg)
            .statusBarsPadding()
            .navigationBarsPadding()
            .imePadding(),
    ) {
        TopBar(state, host, onDrawMode = {
            if (state.tool == InkTool.TYPE) {
                keyboard?.hide()
                focus.clearFocus()
                state.tool = InkTool.PEN
            } else {
                state.tool = InkTool.TYPE
            }
        }, onSheet = { sheet = it })
        if (state.trashed) TrashBanner(state, host)
        BoxWithConstraints(Modifier.weight(1f).fillMaxWidth()) {
            val density = LocalDensity.current
            val width = constraints.maxWidth.toFloat()
            val scale = remember(width, density) { PageScale(width / PAGE.width, density) }
            val scroll = rememberScrollState()
            var front by remember { mutableStateOf<FrontInk?>(null) }
            if (!state.trashed) {
                // Laid out under the page, so touches reach the page first,
                // but shown above it.
                AndroidView(
                    factory = { context -> frontInkSurface(context) { front = it } },
                    onRelease = { front?.release() },
                    modifier = Modifier.matchParentSize().onGloballyPositioned { front?.coords = it },
                )
            }
            CompositionLocalProvider(LocalPage provides scale) {
                Column(Modifier.fillMaxSize().verticalScroll(scroll)) {
                    Column(Modifier.padding(horizontal = scale.dp(PAGE.margin))) {
                        TitleField(state)
                        Text(
                            state.info?.let { dateLabel(it.updatedAt) } ?: "",
                            color = c.textFaint,
                            fontSize = 13.sp,
                            modifier = Modifier.padding(top = 2.dp, bottom = 6.dp),
                        )
                    }
                    PageBody(state, scale, scroll, front)
                }
            }
        }
        if (!state.trashed) {
            HorizontalDivider(color = c.border)
            if (state.tool == InkTool.TYPE) TextToolbar(state) else InkToolbar(state)
        }
    }

    when (sheet) {
        Sheet.COLOR -> NoteColorDialog(state) { sheet = Sheet.NONE }
        Sheet.MOVE -> MoveDialog(state, host.folders()) { sheet = Sheet.NONE }
        Sheet.DELETE -> AlertDialog(
            onDismissRequest = { sheet = Sheet.NONE },
            title = { Text("Delete forever?") },
            text = { Text("This note and its pictures will be gone for good.") },
            confirmButton = {
                TextButton(onClick = {
                    sheet = Sheet.NONE
                    state.deleteForever()
                    host.close()
                }) { Text("Delete", color = c.danger) }
            },
            dismissButton = { TextButton(onClick = { sheet = Sheet.NONE }) { Text("Cancel") } },
        )
        Sheet.NONE -> {}
    }
}

@Composable
private fun TopBar(state: EditorState, host: EditorHost, onDrawMode: () -> Unit, onSheet: (Sheet) -> Unit) {
    val c = Snot.colors
    val context = LocalContext.current
    var menu by remember { mutableStateOf(false) }
    Row(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 4.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        ToolButton(SnotIcons.ArrowLeft, "Back") { host.close() }
        Spacer(Modifier.weight(1f))
        if (!state.trashed) {
            ToolButton(SnotIcons.Undo, "Undo", enabled = state.canUndo) { state.undo() }
            ToolButton(SnotIcons.Redo, "Redo", enabled = state.canRedo) { state.redo() }
            ToolButton(SnotIcons.Pen, "Draw", active = state.tool != InkTool.TYPE, onClick = onDrawMode)
        }
        Box {
            ToolButton(SnotIcons.More, "More") { menu = true }
            DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                val info = state.info
                if (!state.trashed) {
                    MenuItem(if (info?.favorite == true) "Remove from favourites" else "Add to favourites",
                        if (info?.favorite == true) SnotIcons.StarFilled else SnotIcons.Star) {
                        menu = false
                        state.setFavorite(info?.favorite != true)
                    }
                    MenuItem(if (info?.pinned == true) "Unpin" else "Pin to top", SnotIcons.Pin) {
                        menu = false
                        state.setPinned(info?.pinned != true)
                    }
                    MenuItem("Note colour", SnotIcons.Palette) {
                        menu = false
                        onSheet(Sheet.COLOR)
                    }
                    MenuItem("Move to folder", SnotIcons.Move) {
                        menu = false
                        onSheet(Sheet.MOVE)
                    }
                    MenuItem("Share as text", SnotIcons.Share) {
                        menu = false
                        host.share(state.title.text, state.markdown())
                    }
                    MenuItem("Move to trash", SnotIcons.Trash, c.danger) {
                        menu = false
                        state.trash()
                        Toast.makeText(context, "Moved to the trash", Toast.LENGTH_SHORT).show()
                        host.close()
                    }
                } else {
                    MenuItem("Restore", SnotIcons.Restore) {
                        menu = false
                        state.restore()
                        host.reopen()
                    }
                    MenuItem("Delete forever", SnotIcons.Trash, c.danger) {
                        menu = false
                        onSheet(Sheet.DELETE)
                    }
                }
            }
        }
    }
}

@Composable
private fun MenuItem(label: String, icon: ImageVector, color: Color = Snot.colors.text, onClick: () -> Unit) {
    DropdownMenuItem(
        text = { Text(label, color = color) },
        leadingIcon = { Icon(icon, null, tint = color, modifier = Modifier.size(20.dp)) },
        onClick = onClick,
    )
}

@Composable
private fun TrashBanner(state: EditorState, host: EditorHost) {
    val c = Snot.colors
    Row(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 4.dp)
            .clip(RoundedCornerShape(10.dp))
            .background(c.accentSoft)
            .padding(start = 14.dp, end = 4.dp, top = 4.dp, bottom = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text("This note is in the trash.", color = c.text, fontSize = 14.sp, modifier = Modifier.weight(1f))
        TextButton(onClick = {
            state.restore()
            host.reopen()
        }) { Text("Restore") }
    }
}

@Composable
private fun TitleField(state: EditorState) {
    val c = Snot.colors
    val requester = remember { FocusRequester() }
    // A brand-new note starts with its title.
    LaunchedEffect(Unit) { if (state.isBlank() && state.tool == InkTool.TYPE) requester.requestFocus() }
    val first = state.blocks.firstOrNull { it.kind is BlockKind.Text }
    BasicTextField(
        value = state.title,
        onValueChange = { state.editTitle(it) },
        readOnly = state.trashed,
        singleLine = true,
        textStyle = TextStyle(color = c.text, fontSize = 26.sp, fontWeight = FontWeight.SemiBold),
        cursorBrush = SolidColor(c.accent),
        keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences, imeAction = ImeAction.Next),
        keyboardActions = androidx.compose.foundation.text.KeyboardActions(onNext = {
            first?.let { state.pendingCaret = app.snot.ffi.CaretInfo(it.id, 0) }
        }),
        modifier = Modifier
            .fillMaxWidth()
            .padding(top = 8.dp)
            .focusRequester(requester)
            .onFocusChanged { if (it.isFocused) state.focused = null },
        decorationBox = { inner ->
            Box {
                if (state.title.text.isEmpty()) Text("Title", color = c.textFaint, fontSize = 26.sp, fontWeight = FontWeight.SemiBold)
                inner()
            }
        },
    )
}

/**
 * The page: blocks flowing down it, with the handwriting over all of it.
 * It runs a screen's worth past the lowest text or writing, so there is
 * always room to carry on.
 */
@Composable
private fun PageBody(state: EditorState, scale: PageScale, scroll: androidx.compose.foundation.ScrollState, front: FrontInk?) {
    var blocksHeight by remember { mutableIntStateOf(0) }
    val room = with(LocalDensity.current) {
        maxOf(PAGE.room * scale.px, (state.inkBottom + PAGE.room) * scale.px - blocksHeight).toDp()
    }
    Column(Modifier.fillMaxWidth().pageInk(state, scale, scroll, front)) {
        Column(
            Modifier
                .fillMaxWidth()
                .onSizeChanged { blocksHeight = it.height }
                .padding(horizontal = scale.dp(PAGE.margin)),
        ) {
            for (b in state.blocks) {
                androidx.compose.runtime.key(b.id) { Block(state, b) }
            }
        }
        // The empty page below the last block: a tap there types at the end.
        Box(
            Modifier
                .fillMaxWidth()
                .height(room)
                .clickable(
                    enabled = !state.trashed && state.tool == InkTool.TYPE,
                    interactionSource = null,
                    indication = null,
                ) { state.caretAtEnd() },
        )
    }
}

@Composable
private fun Block(state: EditorState, b: BlockView) {
    when (val k = b.kind) {
        is BlockKind.Text -> TextBlock(state, b.id, k)
        is BlockKind.Divider -> DividerBlock(state, b.id)
        is BlockKind.Image -> ImageBlock(state, b.id, k)
    }
}

@Composable
private fun TextBlock(state: EditorState, id: String, k: BlockKind.Text) {
    val c = Snot.colors
    val page = LocalPage.current
    val value = state.fields[id] ?: return
    val m = TEXT.getValue(k.style)
    val requester = remember { FocusRequester() }
    val pending = state.pendingCaret
    LaunchedEffect(pending) {
        if (pending != null && pending.block == id) {
            state.pendingCaret = null
            val at = value.text.utf16Of(pending.offset)
            state.fields[id] = value.copy(selection = TextRange(at))
            requester.requestFocus()
        }
    }
    val color = when {
        k.style == StyleArg.CHECKLIST && k.checked -> c.textFaint
        k.style == StyleArg.QUOTE -> c.textMuted
        else -> c.text
    }
    val style = TextStyle(
        color = color,
        fontSize = page.sp(m.size),
        lineHeight = page.sp(m.lineHeight),
        fontWeight = if (m.semibold) FontWeight.SemiBold else FontWeight.Normal,
        fontFamily = if (m.mono) FontFamily.Monospace else FontFamily.Default,
        textDecoration = if (k.style == StyleArg.CHECKLIST && k.checked) TextDecoration.LineThrough else null,
    )
    val lineHeight = page.dp(m.lineHeight)
    Row(
        Modifier
            .fillMaxWidth()
            .height(IntrinsicSize.Min)
            .padding(start = page.dp(k.indent * PAGE.indent), top = page.dp(m.top), bottom = page.dp(m.bottom))
            .then(
                if (k.style == StyleArg.CODE) {
                    Modifier
                        .clip(RoundedCornerShape(page.dp(15f)))
                        .background(c.codeBg)
                        .padding(horizontal = page.dp(PAGE.codePadX), vertical = page.dp(PAGE.codePadY))
                } else {
                    Modifier
                },
            ),
    ) {
        when (k.style) {
            StyleArg.BULLET -> Prefix(lineHeight) {
                Box(Modifier.size(page.dp(PAGE.bullet)).clip(CircleShape).background(c.text))
            }
            StyleArg.NUMBERED -> Prefix(lineHeight) {
                Text("${k.number ?: 1}.", color = c.textMuted, fontSize = page.sp(m.size))
            }
            StyleArg.CHECKLIST -> Prefix(lineHeight) {
                val corner = RoundedCornerShape(page.dp(12f))
                Box(
                    Modifier
                        .size(page.dp(PAGE.checkbox))
                        .clip(corner)
                        .background(if (k.checked) c.accent else Color.Transparent)
                        .border(page.dp(3.5f), if (k.checked) c.accent else c.textMuted, corner)
                        .clickable(enabled = !state.trashed) { state.toggleChecked(id) },
                    contentAlignment = Alignment.Center,
                ) {
                    if (k.checked) Icon(SnotIcons.Check, "Done", tint = c.accentText, modifier = Modifier.size(page.dp(36f)))
                }
            }
            StyleArg.QUOTE -> Box(
                Modifier
                    .padding(end = page.dp(PAGE.quoteGap))
                    .width(page.dp(PAGE.quoteBar))
                    .fillMaxHeight()
                    .clip(RoundedCornerShape(page.dp(4f)))
                    .background(c.quoteBar),
            )
            else -> {}
        }
        BasicTextField(
            value = value,
            onValueChange = { state.edit(id, it) },
            readOnly = state.trashed,
            textStyle = style,
            cursorBrush = SolidColor(c.accent),
            keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
            modifier = Modifier
                .weight(1f)
                .focusRequester(requester)
                .onFocusChanged { if (it.isFocused) state.focused = id }
                .onPreviewKeyEvent { e ->
                    val sel = value.selection
                    if (e.type != KeyEventType.KeyDown || !sel.collapsed || state.trashed) return@onPreviewKeyEvent false
                    when {
                        e.key == Key.Backspace && sel.start == 0 -> {
                            state.backspaceAtStart(id)
                            true
                        }
                        e.key == Key.Delete && sel.start == value.text.length -> {
                            state.deleteAtEnd(id)
                            true
                        }
                        else -> false
                    }
                },
            decorationBox = { inner ->
                Box {
                    if (value.text.isEmpty() && state.blocks.size == 1 && !state.trashed) {
                        Text("Start writing", style = style.copy(color = c.textFaint))
                    }
                    inner()
                }
            },
        )
    }
}

@Composable
private fun Prefix(lineHeight: androidx.compose.ui.unit.Dp, content: @Composable () -> Unit) {
    Box(Modifier.width(LocalPage.current.dp(PAGE.prefix)).height(lineHeight), contentAlignment = Alignment.CenterStart) {
        content()
    }
}

/** Long-press menu for blocks that are not text: reorder and delete. */
@Composable
private fun BlockMenu(
    state: EditorState,
    id: String,
    expanded: Boolean,
    onDismiss: () -> Unit,
    extra: @Composable (close: () -> Unit) -> Unit = {},
) {
    DropdownMenu(expanded = expanded, onDismissRequest = onDismiss) {
        extra(onDismiss)
        MenuItem("Move up", SnotIcons.ArrowUp) {
            onDismiss()
            state.moveBlock(id, -1)
        }
        MenuItem("Move down", SnotIcons.ArrowDown) {
            onDismiss()
            state.moveBlock(id, 1)
        }
        MenuItem("Delete", SnotIcons.Trash, Snot.colors.danger) {
            onDismiss()
            state.deleteBlock(id)
        }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun DividerBlock(state: EditorState, id: String) {
    var menu by remember { mutableStateOf(false) }
    Box(
        Modifier
            .fillMaxWidth()
            .height(LocalPage.current.dp(PAGE.dividerHeight))
            .combinedClickable(enabled = !state.trashed, onClick = {}, onLongClick = { menu = true }),
        contentAlignment = Alignment.Center,
    ) {
        HorizontalDivider(color = Snot.colors.border, thickness = 1.5.dp)
        BlockMenu(state, id, menu, { menu = false })
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun ImageBlock(state: EditorState, id: String, k: BlockKind.Image) {
    val c = Snot.colors
    val page = LocalPage.current
    var menu by remember { mutableStateOf(false) }
    val bitmap by produceState<android.graphics.Bitmap?>(null, k.attachment) {
        value = withContext(Dispatchers.IO) { decodeScaled(state.attachmentPath(k.attachment), 1600) }
    }
    Column(Modifier.fillMaxWidth().padding(vertical = page.dp(PAGE.imageGap))) {
        Box(
            Modifier
                .fillMaxWidth()
                .aspectRatio(1f / k.aspect.coerceIn(0.05f, 20f))
                .clip(RoundedCornerShape(page.dp(20f)))
                .background(c.codeBg)
                .combinedClickable(enabled = !state.trashed, onClick = {}, onLongClick = { menu = true }),
        ) {
            bitmap?.let {
                Image(it.asImageBitmap(), k.caption.ifEmpty { "Picture" }, Modifier.fillMaxSize(), contentScale = ContentScale.Fit)
            }
            BlockMenu(state, id, menu, { menu = false })
        }
        if (k.caption.isNotEmpty()) {
            Text(
                k.caption,
                color = c.textMuted,
                fontSize = page.sp(PAGE.captionSize),
                lineHeight = page.sp(PAGE.captionSize * PAGE.lineHeight),
                fontStyle = FontStyle.Italic,
                modifier = Modifier.padding(top = page.dp(15f)),
            )
        }
    }
}

/** Decodes a picture no wider than about [maxWidth] pixels. */
private fun decodeScaled(path: String, maxWidth: Int): android.graphics.Bitmap? {
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeFile(path, bounds)
    var sample = 1
    while (bounds.outWidth / (sample * 2) >= maxWidth) sample *= 2
    return BitmapFactory.decodeFile(path, BitmapFactory.Options().apply { inSampleSize = sample })
}

// ---- toolbars ----------------------------------------------------------------

private enum class Pop { NONE, STYLE, HIGHLIGHT, COLOR, INSERT }

@Composable
private fun TextToolbar(state: EditorState) {
    val c = Snot.colors
    val context = LocalContext.current
    var pop by remember { mutableStateOf(Pop.NONE) }
    val style = state.focusedStyle()
    // Re-read when the selection or text moves.
    val marks = remember(state.focused, state.fields[state.focused ?: ""], state.pendingMarks, state.version) {
        state.activeMarks()
    }
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri: Uri? ->
        if (uri == null) return@rememberLauncherForActivityResult
        try {
            val bytes = context.contentResolver.openInputStream(uri)?.use { it.readBytes() } ?: return@rememberLauncherForActivityResult
            state.addPicture(bytes, displayName(context, uri) ?: "picture")
        } catch (e: Exception) {
            Toast.makeText(context, e.message ?: "Could not add that picture.", Toast.LENGTH_LONG).show()
        }
    }

    Row(
        Modifier
            .fillMaxWidth()
            .horizontalScroll(rememberScrollState())
            .padding(horizontal = 6.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box {
            ToolButton(SnotIcons.Plus, "Insert") { pop = Pop.INSERT }
            DropdownMenu(expanded = pop == Pop.INSERT, onDismissRequest = { pop = Pop.NONE }) {
                MenuItem("Picture", SnotIcons.Image) {
                    pop = Pop.NONE
                    picker.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly))
                }
                MenuItem("Divider", SnotIcons.Divider) {
                    pop = Pop.NONE
                    state.insertDivider()
                }
            }
        }
        Sep()
        val focused = style != null
        Box {
            val heading = style in listOf(StyleArg.HEADING1, StyleArg.HEADING2, StyleArg.HEADING3, StyleArg.QUOTE, StyleArg.CODE)
            ToolButton(SnotIcons.Heading, "Paragraph style", active = heading, enabled = focused) { pop = Pop.STYLE }
            DropdownMenu(expanded = pop == Pop.STYLE, onDismissRequest = { pop = Pop.NONE }) {
                for ((s, label, size) in listOf(
                    Triple(StyleArg.BODY, "Body text", 16),
                    Triple(StyleArg.HEADING1, "Heading 1", 22),
                    Triple(StyleArg.HEADING2, "Heading 2", 19),
                    Triple(StyleArg.HEADING3, "Heading 3", 17),
                    Triple(StyleArg.QUOTE, "Quote", 16),
                    Triple(StyleArg.CODE, "Code", 15),
                )) {
                    DropdownMenuItem(
                        text = {
                            Text(
                                label,
                                fontSize = size.sp,
                                fontWeight = if (size > 16) FontWeight.SemiBold else FontWeight.Normal,
                                fontFamily = if (s == StyleArg.CODE) FontFamily.Monospace else FontFamily.Default,
                                color = if (style == s) c.accent else c.text,
                            )
                        },
                        onClick = {
                            pop = Pop.NONE
                            if (style != s) state.toggleStyle(s)
                        },
                    )
                }
            }
        }
        ToolButton(SnotIcons.Bullets, "Bulleted list", active = style == StyleArg.BULLET, enabled = focused) { state.toggleStyle(StyleArg.BULLET) }
        ToolButton(SnotIcons.Numbers, "Numbered list", active = style == StyleArg.NUMBERED, enabled = focused) { state.toggleStyle(StyleArg.NUMBERED) }
        ToolButton(SnotIcons.Checklist, "Checklist", active = style == StyleArg.CHECKLIST, enabled = focused) { state.toggleStyle(StyleArg.CHECKLIST) }
        Sep()
        ToolButton(SnotIcons.Bold, "Bold", active = MarkArg.Bold in marks, enabled = focused) { state.toggleMark(MarkArg.Bold) }
        ToolButton(SnotIcons.Italic, "Italic", active = MarkArg.Italic in marks, enabled = focused) { state.toggleMark(MarkArg.Italic) }
        ToolButton(SnotIcons.Underline, "Underline", active = MarkArg.Underline in marks, enabled = focused) { state.toggleMark(MarkArg.Underline) }
        ToolButton(SnotIcons.Strike, "Strikethrough", active = MarkArg.Strike in marks, enabled = focused) { state.toggleMark(MarkArg.Strike) }
        Box {
            val current = marks.filterIsInstance<MarkArg.Highlight>().firstOrNull()
            ToolButton(SnotIcons.Highlighter, "Highlight", active = current != null, enabled = focused) { pop = Pop.HIGHLIGHT }
            DropdownMenu(expanded = pop == Pop.HIGHLIGHT, onDismissRequest = { pop = Pop.NONE }) {
                SwatchRow(highlightColors(), current?.color, { argb(highlightColor(it, state.dark)) }) {
                    pop = Pop.NONE
                    state.toggleMark(MarkArg.Highlight(it))
                }
            }
        }
        Box {
            val current = marks.filterIsInstance<MarkArg.Color>().firstOrNull()
            ToolButton(SnotIcons.Palette, "Text colour", active = current != null, enabled = focused) { pop = Pop.COLOR }
            DropdownMenu(expanded = pop == Pop.COLOR, onDismissRequest = { pop = Pop.NONE }) {
                SwatchRow(inkColors().filter { it != "ink" }, current?.color, { argb(inkColor(it, state.dark)) }) {
                    pop = Pop.NONE
                    state.toggleMark(MarkArg.Color(it))
                }
                DropdownMenuItem(text = { Text("Clear formatting") }, onClick = {
                    pop = Pop.NONE
                    state.clearMarks()
                })
            }
        }
        Sep()
        ToolButton(SnotIcons.Outdent, "Outdent", enabled = focused) { state.indent(-1) }
        ToolButton(SnotIcons.Indent, "Indent", enabled = focused) { state.indent(1) }
    }
}

/** Colour swatches laid out in rows of five inside a menu. */
@Composable
private fun SwatchRow(tokens: List<String>, selected: String?, color: (String) -> Color, onPick: (String) -> Unit) {
    Column(Modifier.padding(horizontal = 8.dp)) {
        for (row in tokens.chunked(5)) {
            Row {
                for (t in row) Swatch(color(t), colorName(t), t == selected, size = 28.dp) { onPick(t) }
            }
        }
    }
}

@Composable
private fun Sep() {
    VerticalDivider(Modifier.height(24.dp).padding(horizontal = 4.dp), color = Snot.colors.border)
}

@Composable
private fun InkToolbar(state: EditorState) {
    val c = Snot.colors
    val tool = state.tool
    var paperMenu by remember { mutableStateOf(false) }
    Column(Modifier.fillMaxWidth().padding(horizontal = 6.dp, vertical = 4.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            ToolButton(SnotIcons.Type, "Type") { state.tool = InkTool.TYPE }
            Box {
                ToolButton(SnotIcons.Lined, "Paper") { paperMenu = true }
                DropdownMenu(expanded = paperMenu, onDismissRequest = { paperMenu = false }) {
                    for ((paper, label, icon) in listOf(
                        Triple(PaperArg.PLAIN, "Plain paper", SnotIcons.Plain),
                        Triple(PaperArg.LINED, "Lined paper", SnotIcons.Lined),
                        Triple(PaperArg.GRID, "Grid paper", SnotIcons.Grid),
                        Triple(PaperArg.DOTTED, "Dotted paper", SnotIcons.Dotted),
                    )) {
                        MenuItem(label, icon, if (state.paper == paper) c.accent else c.text) {
                            paperMenu = false
                            state.choosePaper(paper)
                        }
                    }
                    HorizontalDivider(color = c.border)
                    MenuItem("Erase all writing", SnotIcons.Eraser) {
                        paperMenu = false
                        state.clearInk()
                    }
                }
            }
            Sep()
            ToolButton(SnotIcons.Pen, "Pen", active = tool == InkTool.PEN) { state.tool = InkTool.PEN }
            ToolButton(SnotIcons.Marker, "Marker", active = tool == InkTool.MARKER) { state.tool = InkTool.MARKER }
            ToolButton(SnotIcons.Highlighter, "Highlighter", active = tool == InkTool.HIGHLIGHTER) { state.tool = InkTool.HIGHLIGHTER }
            ToolButton(SnotIcons.Eraser, "Eraser", active = tool == InkTool.ERASER) { state.tool = InkTool.ERASER }
            Spacer(Modifier.weight(1f))
            val pen = state.pens.getValue(tool)
            val max = tool.widths.maxOrNull() ?: 1f
            for (w in tool.widths) {
                val dot = (6 + 16 * w / max).dp
                Box(
                    Modifier
                        .size(36.dp)
                        .clip(CircleShape)
                        .background(if (pen.width == w) c.accentSoft else Color.Transparent)
                        .clickable { state.pens[tool] = pen.copy(width = w) },
                    contentAlignment = Alignment.Center,
                ) {
                    Box(
                        Modifier
                            .size(dot)
                            .clip(CircleShape)
                            .then(
                                if (tool == InkTool.ERASER) {
                                    Modifier.border(1.5.dp, c.textMuted, CircleShape)
                                } else {
                                    Modifier.background(if (pen.width == w) c.accent else c.textMuted)
                                },
                            ),
                    )
                }
            }
        }
        if (tool != InkTool.ERASER) {
            val pen = state.pens.getValue(tool)
            val tokens = if (tool == InkTool.HIGHLIGHTER) highlightColors() else inkColors()
            Row(
                Modifier
                    .fillMaxWidth()
                    .horizontalScroll(rememberScrollState()),
                horizontalArrangement = Arrangement.spacedBy(0.dp),
            ) {
                for (t in tokens) {
                    val color = if (tool == InkTool.HIGHLIGHTER) highlightColor(t, state.dark) else inkColor(t, state.dark)
                    Swatch(argb(color), colorName(t), pen.color == t, size = 26.dp) {
                        state.pens[tool] = pen.copy(color = t)
                    }
                }
            }
        }
    }
}

// ---- dialogs -----------------------------------------------------------------

@Composable
private fun NoteColorDialog(state: EditorState, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Note colour") },
        text = {
            Column {
                for (row in (listOf<String?>(null) + noteColors()).chunked(4)) {
                    Row {
                        for (t in row) {
                            Swatch(argb(noteBackground(t, state.dark)), t?.let(::colorName) ?: "None", state.info?.color == t, size = 40.dp) {
                                state.setColor(t)
                                onDismiss()
                            }
                        }
                    }
                }
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text("Done") } },
    )
}

@Composable
private fun MoveDialog(state: EditorState, folders: List<FolderInfo>, onDismiss: () -> Unit) {
    val c = Snot.colors
    val current = state.info?.folderId
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Move to folder") },
        text = {
            Column(Modifier.widthIn(min = 240.dp).verticalScroll(rememberScrollState())) {
                for ((id, name) in listOf<Pair<String?, String>>(null to "No folder") + folders.map { it.id to it.name }) {
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .clip(RoundedCornerShape(8.dp))
                            .clickable {
                                state.moveTo(id)
                                onDismiss()
                            }
                            .padding(horizontal = 8.dp, vertical = 12.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Icon(if (id == null) SnotIcons.Inbox else SnotIcons.Folder, null, tint = c.textMuted, modifier = Modifier.size(20.dp))
                        Text(name, color = if (id == current) c.accent else c.text, modifier = Modifier.padding(start = 12.dp))
                    }
                }
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

private fun displayName(context: android.content.Context, uri: Uri): String? =
    context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
        if (it.moveToFirst()) it.getString(0) else null
    }
