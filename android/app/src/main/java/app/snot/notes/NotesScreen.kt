package app.snot.notes

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.lazy.staggeredgrid.LazyVerticalStaggeredGrid
import androidx.compose.foundation.lazy.staggeredgrid.StaggeredGridCells
import androidx.compose.foundation.lazy.staggeredgrid.StaggeredGridItemSpan
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DrawerValue
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalDrawerSheet
import androidx.compose.material3.ModalNavigationDrawer
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.SmallFloatingActionButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberDrawerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import app.snot.editor.snippet
import app.snot.ffi.NoteInfo
import app.snot.ffi.ScopeArg
import app.snot.ffi.SortArg
import app.snot.ffi.highlightColor
import app.snot.ffi.noteBackground
import app.snot.ffi.noteColors
import app.snot.ui.Snot
import app.snot.ui.SnotIcons
import app.snot.ui.Swatch
import app.snot.ui.ToolButton
import app.snot.ui.argb
import app.snot.ui.colorName
import app.snot.ui.dateLabel
import kotlinx.coroutines.launch

/** What the list asks of the rest of the app. */
interface NotesHost {
    fun open(id: String)
    fun create(handwriting: Boolean)
    fun importMarkdown()
}

private sealed interface Dialog {
    data object NewFolder : Dialog
    data class RenameFolder(val id: String, val name: String) : Dialog
    data class DeleteFolder(val id: String, val name: String) : Dialog
    data object EmptyTrash : Dialog
    data class DeleteNote(val note: NoteInfo) : Dialog
    data class NoteColor(val note: NoteInfo) : Dialog
    data class Move(val note: NoteInfo) : Dialog
}

@Composable
fun NotesScreen(state: LibraryState, host: NotesHost) {
    val c = Snot.colors
    val drawer = rememberDrawerState(DrawerValue.Closed)
    val scope = rememberCoroutineScope()
    var dialog by remember { mutableStateOf<Dialog?>(null) }

    ModalNavigationDrawer(
        drawerState = drawer,
        drawerContent = {
            Drawer(state, onPick = {
                state.show(it)
                scope.launch { drawer.close() }
            }, onDialog = {
                dialog = it
                scope.launch { drawer.close() }
            }, onImport = {
                scope.launch { drawer.close() }
                host.importMarkdown()
            })
        },
    ) {
        Box(
            Modifier
                .fillMaxSize()
                .background(c.listBg),
        ) {
            Column(Modifier.fillMaxSize().statusBarsPadding()) {
                Header(state, onMenu = { scope.launch { drawer.open() } }, onDialog = { dialog = it })
                NoteGrid(state, host, onDialog = { dialog = it })
            }
            if (state.scope != ScopeArg.Trash && !state.searching) {
                Column(
                    Modifier
                        .align(Alignment.BottomEnd)
                        .navigationBarsPadding()
                        .padding(20.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(14.dp),
                ) {
                    SmallFloatingActionButton(
                        onClick = { host.create(handwriting = true) },
                        containerColor = c.cardBg,
                        contentColor = c.accent,
                    ) { Icon(SnotIcons.Pen, "New handwriting note") }
                    FloatingActionButton(
                        onClick = { host.create(handwriting = false) },
                        containerColor = c.accent,
                        contentColor = c.accentText,
                    ) { Icon(SnotIcons.Plus, "New note") }
                }
            }
        }
    }

    dialog?.let { d -> Dialogs(state, d) { dialog = null } }
}

@Composable
private fun Header(state: LibraryState, onMenu: () -> Unit, onDialog: (Dialog) -> Unit) {
    val c = Snot.colors
    var sortMenu by remember { mutableStateOf(false) }
    Row(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 4.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (state.searching) {
            ToolButton(SnotIcons.ArrowLeft, "Stop searching") { state.endSearch() }
            val focus = remember { FocusRequester() }
            LaunchedEffect(Unit) { focus.requestFocus() }
            BasicTextField(
                value = state.query,
                onValueChange = { state.search(it) },
                singleLine = true,
                textStyle = TextStyle(color = c.text, fontSize = 18.sp),
                cursorBrush = SolidColor(c.accent),
                modifier = Modifier
                    .weight(1f)
                    .padding(horizontal = 8.dp)
                    .focusRequester(focus),
                decorationBox = { inner ->
                    Box {
                        if (state.query.isEmpty()) Text("Search notes", color = c.textFaint, fontSize = 18.sp)
                        inner()
                    }
                },
            )
            if (state.query.isNotEmpty()) ToolButton(SnotIcons.Close, "Clear") { state.search("") }
            return@Row
        }
        ToolButton(SnotIcons.Menu, "Folders", onClick = onMenu)
        Column(Modifier.weight(1f).padding(start = 8.dp)) {
            Text(state.title, color = c.text, fontSize = 22.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
            val n = state.notes.size
            Text(if (n == 1) "1 note" else "$n notes", color = c.textMuted, fontSize = 13.sp)
        }
        ToolButton(SnotIcons.Search, "Search") { state.startSearch() }
        if (state.scope == ScopeArg.Trash) {
            ToolButton(SnotIcons.Trash, "Empty trash", enabled = state.notes.isNotEmpty()) { onDialog(Dialog.EmptyTrash) }
        } else {
            Box {
                ToolButton(SnotIcons.Sort, "Sort") { sortMenu = true }
                DropdownMenu(expanded = sortMenu, onDismissRequest = { sortMenu = false }) {
                    for ((s, label) in listOf(
                        SortArg.MODIFIED to "Date modified",
                        SortArg.CREATED to "Date created",
                        SortArg.TITLE to "Title",
                    )) {
                        DropdownMenuItem(
                            text = { Text(label, color = if (state.sort == s) c.accent else c.text) },
                            trailingIcon = { if (state.sort == s) Icon(SnotIcons.Check, null, tint = c.accent) },
                            onClick = {
                                sortMenu = false
                                state.sortBy(s)
                            },
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun NoteGrid(state: LibraryState, host: NotesHost, onDialog: (Dialog) -> Unit) {
    val c = Snot.colors
    if (state.notes.isEmpty()) {
        Box(Modifier.fillMaxSize().padding(32.dp), contentAlignment = Alignment.Center) {
            Text(
                when {
                    state.searching && state.query.isNotBlank() -> "No notes match “${state.query}”."
                    state.searching -> ""
                    state.scope == ScopeArg.Trash -> "The trash is empty."
                    state.scope == ScopeArg.Favorites -> "Star a note to keep it here."
                    else -> "No notes yet. Tap + to write one."
                },
                color = c.textMuted,
                fontSize = 15.sp,
            )
        }
        return
    }
    LazyVerticalStaggeredGrid(
        columns = StaggeredGridCells.Adaptive(160.dp),
        state = state.grid,
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(start = 12.dp, end = 12.dp, top = 4.dp, bottom = 120.dp),
        verticalItemSpacing = 10.dp,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        if (state.scope == ScopeArg.Trash && !state.searching) {
            item(span = StaggeredGridItemSpan.FullLine) {
                Text(
                    "Notes in the trash are deleted after 30 days.",
                    color = c.textMuted,
                    fontSize = 13.sp,
                    modifier = Modifier.padding(horizontal = 4.dp, vertical = 4.dp),
                )
            }
        }
        items(state.notes.size, key = { state.notes[it].id }) { i ->
            NoteCard(state, state.notes[i], host, onDialog)
        }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun NoteCard(state: LibraryState, n: NoteInfo, host: NotesHost, onDialog: (Dialog) -> Unit) {
    val c = Snot.colors
    val dark = isSystemInDarkTheme()
    var menu by remember { mutableStateOf(false) }
    val bg = if (n.color != null) argb(noteBackground(n.color, dark)) else c.cardBg
    val trashed = n.trashedAt != null
    Box {
        Column(
            Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(14.dp))
                .background(bg)
                .border(1.dp, c.border, RoundedCornerShape(14.dp))
                .combinedClickable(onClick = { host.open(n.id) }, onLongClick = { menu = true })
                .padding(14.dp),
        ) {
            Row(verticalAlignment = Alignment.Top) {
                Text(
                    n.title.ifEmpty { "Untitled" },
                    color = if (n.title.isEmpty()) c.textMuted else c.text,
                    fontSize = 16.sp,
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
                if (n.pinned) Icon(SnotIcons.Pin, "Pinned", tint = c.accent, modifier = Modifier.padding(start = 4.dp).size(16.dp))
                if (n.favorite) Icon(SnotIcons.StarFilled, "Favourite", tint = c.accent, modifier = Modifier.padding(start = 4.dp).size(16.dp))
            }
            val body = n.snippet?.let { snippet(it, SpanStyle(background = argb(highlightColor("yellow", dark)), color = c.text)) }
            if (body != null || n.preview.isNotBlank()) {
                Text(
                    body ?: androidx.compose.ui.text.AnnotatedString(n.preview),
                    color = c.textMuted,
                    fontSize = 14.sp,
                    lineHeight = 19.sp,
                    maxLines = 7,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.padding(top = 6.dp),
                )
            }
            Spacer(Modifier.height(10.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(dateLabel(if (trashed) n.trashedAt!! else n.updatedAt), color = c.textFaint, fontSize = 12.sp, modifier = Modifier.weight(1f))
                if (n.hasInk) Icon(SnotIcons.Drawing, "Has drawing", tint = c.textFaint, modifier = Modifier.padding(start = 4.dp).size(15.dp))
                if (n.hasImage) Icon(SnotIcons.Image, "Has picture", tint = c.textFaint, modifier = Modifier.padding(start = 4.dp).size(15.dp))
            }
        }
        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
            fun pick(f: () -> Unit) {
                menu = false
                f()
            }
            if (trashed) {
                Item("Restore", SnotIcons.Restore) { pick { state.restore(n) } }
                Item("Delete forever", SnotIcons.Trash, c.danger) { pick { onDialog(Dialog.DeleteNote(n)) } }
            } else {
                Item(if (n.pinned) "Unpin" else "Pin to top", SnotIcons.Pin) { pick { state.setPinned(n, !n.pinned) } }
                Item(if (n.favorite) "Remove from favourites" else "Add to favourites", if (n.favorite) SnotIcons.StarFilled else SnotIcons.Star) {
                    pick { state.setFavorite(n, !n.favorite) }
                }
                Item("Note colour", SnotIcons.Palette) { pick { onDialog(Dialog.NoteColor(n)) } }
                Item("Move to folder", SnotIcons.Move) { pick { onDialog(Dialog.Move(n)) } }
                Item("Duplicate", SnotIcons.Copy) { pick { state.duplicate(n) } }
                Item("Move to trash", SnotIcons.Trash, c.danger) { pick { state.trash(n) } }
            }
        }
    }
}

@Composable
private fun Item(label: String, icon: ImageVector, color: Color = Snot.colors.text, onClick: () -> Unit) {
    DropdownMenuItem(
        text = { Text(label, color = color) },
        leadingIcon = { Icon(icon, null, tint = color, modifier = Modifier.size(20.dp)) },
        onClick = onClick,
    )
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun DrawerRow(
    label: String,
    icon: ImageVector,
    count: Long,
    selected: Boolean,
    onLongClick: (() -> Unit)? = null,
    onClick: () -> Unit,
) {
    val c = Snot.colors
    Row(
        Modifier
            .fillMaxWidth()
            .height(52.dp)
            .clip(RoundedCornerShape(26.dp))
            .background(if (selected) c.accentSoft else Color.Transparent)
            .combinedClickable(onClick = onClick, onLongClick = onLongClick)
            .padding(horizontal = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, null, tint = if (selected) c.accent else c.textMuted, modifier = Modifier.size(22.dp))
        Text(
            label,
            color = c.text,
            fontSize = 15.sp,
            fontWeight = if (selected) FontWeight.SemiBold else FontWeight.Normal,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f).padding(start = 14.dp),
        )
        if (count > 0) Text("$count", color = c.textMuted, fontSize = 13.sp)
    }
}

@Composable
private fun Drawer(state: LibraryState, onPick: (ScopeArg) -> Unit, onDialog: (Dialog) -> Unit, onImport: () -> Unit) {
    val c = Snot.colors
    ModalDrawerSheet(drawerContainerColor = c.appBg) {
        Column(Modifier.verticalScroll(rememberScrollState()).padding(horizontal = 12.dp)) {
            Text(
                "Snot",
                color = c.accent,
                fontSize = 24.sp,
                fontWeight = FontWeight.Bold,
                modifier = Modifier.padding(start = 16.dp, top = 20.dp, bottom = 14.dp),
            )
            val counts = state.counts
            for ((label, icon, scope, count) in listOf(
                Entry("All notes", SnotIcons.Notes, ScopeArg.All, counts.all),
                Entry("Favourites", SnotIcons.Star, ScopeArg.Favorites, counts.favorites),
                Entry("No folder", SnotIcons.Inbox, ScopeArg.Unfiled, counts.unfiled),
            )) {
                DrawerRow(label, icon, count, state.scope == scope) { onPick(scope) }
            }

            HorizontalDivider(color = c.border, modifier = Modifier.padding(vertical = 10.dp))
            Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(start = 16.dp)) {
                Text("Folders", color = c.textMuted, fontSize = 13.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f))
                ToolButton(SnotIcons.FolderPlus, "New folder", size = 40.dp) { onDialog(Dialog.NewFolder) }
            }
            for (f in state.folders) {
                var menu by remember(f.id) { mutableStateOf(false) }
                Box {
                    val scope = ScopeArg.Folder(f.id)
                    DrawerRow(f.name, SnotIcons.Folder, f.noteCount, state.scope == scope, onLongClick = { menu = true }) {
                        onPick(scope)
                    }
                    DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                        Item("Rename", SnotIcons.Edit) {
                            menu = false
                            onDialog(Dialog.RenameFolder(f.id, f.name))
                        }
                        Item("Delete folder", SnotIcons.Trash, c.danger) {
                            menu = false
                            onDialog(Dialog.DeleteFolder(f.id, f.name))
                        }
                    }
                }
            }
            if (state.folders.isEmpty()) {
                Text("No folders yet.", color = c.textFaint, fontSize = 14.sp, modifier = Modifier.padding(start = 16.dp, top = 4.dp, bottom = 8.dp))
            }

            HorizontalDivider(color = c.border, modifier = Modifier.padding(vertical = 10.dp))
            DrawerRow("Trash", SnotIcons.Trash, counts.trash, state.scope == ScopeArg.Trash) { onPick(ScopeArg.Trash) }
            DrawerRow("Import Markdown\u2026", SnotIcons.Import, 0, false, onClick = onImport)
            Spacer(Modifier.height(16.dp))
        }
    }
}

private data class Entry(val label: String, val icon: ImageVector, val scope: ScopeArg, val count: Long)

@Composable
private fun Dialogs(state: LibraryState, d: Dialog, close: () -> Unit) {
    val c = Snot.colors
    val dark = isSystemInDarkTheme()
    when (d) {
        Dialog.NewFolder -> NameDialog("New folder", "", "Create", close) { state.createFolder(it) }
        is Dialog.RenameFolder -> NameDialog("Rename folder", d.name, "Rename", close) { state.renameFolder(d.id, it) }
        is Dialog.DeleteFolder -> Confirm(
            "Delete “${d.name}”?",
            "Its notes go to the trash, where you can still restore them.",
            "Delete",
            close,
        ) { state.deleteFolder(d.id) }
        Dialog.EmptyTrash -> Confirm(
            "Empty the trash?",
            if (state.counts.trash == 1L) "1 note will be deleted for good." else "${state.counts.trash} notes will be deleted for good.",
            "Empty trash",
            close,
        ) { state.emptyTrash() }
        is Dialog.DeleteNote -> Confirm(
            "Delete this note forever?",
            "“${d.note.title.ifEmpty { "Untitled" }}” and its pictures will be gone for good.",
            "Delete",
            close,
        ) { state.delete(d.note) }
        is Dialog.NoteColor -> AlertDialog(
            onDismissRequest = close,
            title = { Text("Note colour") },
            text = {
                Column {
                    for (row in (listOf<String?>(null) + noteColors()).chunked(4)) {
                        Row {
                            for (t in row) {
                                Swatch(argb(noteBackground(t, dark)), t?.let(::colorName) ?: "None", d.note.color == t, size = 40.dp) {
                                    state.setColor(d.note, t)
                                    close()
                                }
                            }
                        }
                    }
                }
            },
            confirmButton = { TextButton(onClick = close) { Text("Done") } },
        )
        is Dialog.Move -> AlertDialog(
            onDismissRequest = close,
            title = { Text("Move to folder") },
            text = {
                Column(Modifier.verticalScroll(rememberScrollState())) {
                    for ((id, name) in listOf<Pair<String?, String>>(null to "No folder") + state.folders.map { it.id to it.name }) {
                        Row(
                            Modifier
                                .fillMaxWidth()
                                .clip(RoundedCornerShape(8.dp))
                                .clickable {
                                    state.move(d.note, id)
                                    close()
                                }
                                .padding(horizontal = 8.dp, vertical = 12.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Icon(if (id == null) SnotIcons.Inbox else SnotIcons.Folder, null, tint = c.textMuted, modifier = Modifier.size(20.dp))
                            Text(name, color = if (id == d.note.folderId) c.accent else c.text, modifier = Modifier.padding(start = 12.dp))
                        }
                    }
                }
            },
            confirmButton = { TextButton(onClick = close) { Text("Cancel") } },
        )
    }
}

@Composable
private fun NameDialog(title: String, initial: String, action: String, close: () -> Unit, onDone: (String) -> Unit) {
    var name by remember { mutableStateOf(initial) }
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { focus.requestFocus() }
    AlertDialog(
        onDismissRequest = close,
        title = { Text(title) },
        text = {
            OutlinedTextField(
                value = name,
                onValueChange = { name = it },
                singleLine = true,
                placeholder = { Text("Folder name") },
                modifier = Modifier.focusRequester(focus),
            )
        },
        confirmButton = {
            TextButton(enabled = name.isNotBlank(), onClick = {
                onDone(name.trim())
                close()
            }) { Text(action) }
        },
        dismissButton = { TextButton(onClick = close) { Text("Cancel") } },
    )
}

@Composable
private fun Confirm(title: String, text: String, action: String, close: () -> Unit, onConfirm: () -> Unit) {
    AlertDialog(
        onDismissRequest = close,
        title = { Text(title) },
        text = { Text(text) },
        confirmButton = {
            TextButton(onClick = {
                onConfirm()
                close()
            }) { Text(action, color = Snot.colors.danger) }
        },
        dismissButton = { TextButton(onClick = close) { Text("Cancel") } },
    )
}
