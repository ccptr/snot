package app.snot.editor

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import app.snot.ffi.BlockKind
import app.snot.ffi.BlockView
import app.snot.ffi.CaretInfo
import app.snot.ffi.MarkArg
import app.snot.ffi.NoteEditor
import app.snot.ffi.NoteInfo
import app.snot.ffi.PaperArg
import app.snot.ffi.ScopeArg
import app.snot.ffi.SnotLibrary
import app.snot.ffi.SortArg
import app.snot.ffi.StyleArg
import app.snot.ffi.ToolArg

enum class InkTool { TYPE, PEN, MARKER, HIGHLIGHTER, ERASER;

    val stroke: ToolArg?
        get() = when (this) {
            PEN -> ToolArg.PEN
            MARKER -> ToolArg.MARKER
            HIGHLIGHTER -> ToolArg.HIGHLIGHTER
            TYPE, ERASER -> null
        }

    /** Nib sizes offered, in page units; the eraser's is its radius. */
    val widths: List<Float>
        get() = when (this) {
            PEN -> listOf(2f, 4f, 7f, 12f)
            MARKER -> listOf(5f, 9f, 15f, 24f)
            HIGHLIGHTER -> listOf(18f, 28f, 42f)
            ERASER -> listOf(8f, 16f, 32f)
            TYPE -> emptyList()
        }
}

/** Colour and nib size remembered for one drawing tool. */
data class Pen(val color: String, val width: Float)

/**
 * One open note: the core's edit session plus what Compose needs to show
 * it. Text fields report their whole value after every change; [edit]
 * turns that into the one core edit it amounts to, then everything is
 * re-read from the core, which stays the single source of truth.
 */
class EditorState(
    private val lib: SnotLibrary,
    val editor: NoteEditor,
    dark: Boolean,
) {
    /** Whether the system is dark; ink and text colours follow it. */
    var dark by mutableStateOf(dark)
        private set

    val noteId: String = editor.noteId()
    val trashed: Boolean = editor.isTrashed()

    var blocks: List<BlockView> by mutableStateOf(editor.blocks())
        private set
    val fields = mutableStateMapOf<String, TextFieldValue>()
    var title by mutableStateOf(TextFieldValue(editor.title()))
        private set

    /** Every stroke, outlined, in the order written. */
    var strokes: List<StrokePath> by mutableStateOf(emptyList())
        private set
    /**
     * How many of the newest strokes the front buffer is still showing.
     * The page leaves them out until the front buffer hands them over.
     */
    var frontTail by mutableIntStateOf(0)
    var paper by mutableStateOf(editor.paper())
        private set
    /** The lowest point the handwriting reaches, in page units. */
    var inkBottom by mutableStateOf(0f)
        private set
    /** The core's ink revision that [strokes] shows. */
    private var shownInk = -1L

    var canUndo by mutableStateOf(false)
        private set
    var canRedo by mutableStateOf(false)
        private set

    /** Bumped on every change; the screen saves a moment after it moves. */
    var version by mutableIntStateOf(0)
        private set

    /** The text block the caret is in, if any. */
    var focused: String? by mutableStateOf(null)

    /** Where the caret should go next; the block's field claims it. */
    var pendingCaret: CaretInfo? by mutableStateOf(null)

    /** Marks the next typed text takes, after a format button with nothing selected. */
    var pendingMarks: List<MarkArg>? by mutableStateOf(null)
        private set

    var info: NoteInfo? by mutableStateOf(null)
        private set

    var tool by mutableStateOf(InkTool.TYPE)
    val pens = mutableStateMapOf(
        InkTool.PEN to Pen("ink", 4f),
        InkTool.MARKER to Pen("blue", 9f),
        InkTool.HIGHLIGHTER to Pen("yellow", 28f),
        InkTool.ERASER to Pen("", 16f),
    )

    init {
        refreshInfo()
        sync()
    }

    fun refreshInfo() {
        info = (lib.notes(ScopeArg.All, SortArg.MODIFIED) + lib.notes(ScopeArg.Trash, SortArg.MODIFIED))
            .firstOrNull { it.id == noteId }
    }

    fun text(block: String): BlockKind.Text? =
        blocks.firstOrNull { it.id == block }?.kind as? BlockKind.Text

    /** Re-reads the blocks, keeping each field's caret where its text survived. */
    private fun sync() {
        blocks = editor.blocks()
        val live = HashSet<String>()
        for (b in blocks) {
            val k = b.kind as? BlockKind.Text ?: continue
            live += b.id
            val ann = styled(k.text, k.runs, dark)
            val cur = fields[b.id]
            fields[b.id] = when {
                cur == null -> TextFieldValue(ann, TextRange(k.text.length))
                cur.text == k.text -> cur.copy(annotatedString = ann)
                else -> TextFieldValue(ann, TextRange(cur.selection.end.coerceAtMost(k.text.length)))
            }
        }
        fields.keys.retainAll(live)
        if (editor.inkRevision() != shownInk) reloadInk()
        canUndo = editor.canUndo()
        canRedo = editor.canRedo()
    }

    private fun changed(caret: CaretInfo? = null) {
        sync()
        if (caret != null) pendingCaret = caret
        version++
    }

    private fun reloadInk() {
        strokes = editor.inkPaths(dark).map(::StrokePath)
        frontTail = 0
        paper = editor.paper()
        inkBottom = editor.inkBottom()
        shownInk = editor.inkRevision()
    }

    /**
     * After a stroke is added or erased in place: the text is untouched,
     * so nothing else is re-read. This runs between strokes, where any
     * delay would hold up the next one.
     */
    private fun inkChanged() {
        shownInk = editor.inkRevision()
        canUndo = editor.canUndo()
        canRedo = editor.canRedo()
        version++
    }

    // ---- text ---------------------------------------------------------------

    fun editTitle(value: TextFieldValue) {
        if (value.text != title.text) {
            editor.setTitle(value.text.replace('\n', ' '))
            version++
            canUndo = editor.canUndo()
        }
        title = value
    }

    /** A text field's new value: a caret move, or an edit to apply. */
    fun edit(block: String, new: TextFieldValue) {
        val old = fields[block] ?: return
        if (new.text == old.text) {
            if (new.selection != old.selection) pendingMarks = null
            fields[block] = new.copy(annotatedString = old.annotatedString)
            return
        }
        val (start, end, inserted) = diff(old.text, new.text)
        val from = old.text.cpOf(start)
        val to = old.text.cpOf(end)
        if ('\n' in inserted) {
            pendingMarks = null
            val caret = if (inserted == "\n") {
                // Enter. Splitting rather than pasting lets an empty list
                // item leave the list.
                if (to > from) editor.replace(block, from, to, "", null)
                editor.split(block, from)
            } else {
                editor.paste(block, from, to, inserted)
            }
            changed(caret)
            return
        }
        editor.replace(block, from, to, inserted, pendingMarks.takeIf { inserted.isNotEmpty() })
        fields[block] = new
        changed()
    }

    /** An untitled note with nothing written or drawn in it. */
    fun isBlank(): Boolean = title.text.isBlank() && editor.strokeCount() == 0 &&
        blocks.all { (it.kind as? BlockKind.Text)?.text?.isEmpty() == true }

    /** Puts the caret at the end of the last text block. */
    fun caretAtEnd() {
        val last = blocks.lastOrNull { it.kind is BlockKind.Text } ?: return
        val text = (last.kind as BlockKind.Text).text
        pendingCaret = CaretInfo(last.id, text.codePointCount(0, text.length))
    }

    /** Backspace with the caret at the very start of a block. */
    fun backspaceAtStart(block: String) {
        changed(editor.backspaceAtStart(block))
    }

    /** Forward delete at the very end of a block. */
    fun deleteAtEnd(block: String) {
        changed(editor.deleteAtEnd(block))
    }

    /** The focused block's selection in core offsets. */
    private fun selection(): Triple<String, Int, Int>? {
        val id = focused ?: return null
        val v = fields[id] ?: return null
        return Triple(id, v.text.cpOf(v.selection.min), v.text.cpOf(v.selection.max))
    }

    fun activeMarks(): List<MarkArg> {
        pendingMarks?.let { return it }
        val (id, from, to) = selection() ?: return emptyList()
        return editor.marksIn(id, from, to)
    }

    fun toggleMark(mark: MarkArg) {
        val (id, from, to) = selection() ?: return
        if (from == to) {
            // Nothing selected: the next typed text takes the mark.
            val marks = (pendingMarks ?: editor.marksIn(id, from, from)).toMutableList()
            if (mark in marks) {
                marks.remove(mark)
            } else {
                marks.removeAll { it::class == mark::class }
                marks.add(mark)
            }
            pendingMarks = marks
            return
        }
        editor.toggleMark(id, from, to, mark)
        changed()
    }

    fun clearMarks() {
        val (id, from, to) = selection() ?: return
        pendingMarks = emptyList()
        editor.clearMarks(id, from, to)
        changed()
    }

    fun focusedStyle(): StyleArg? = focused?.let { text(it)?.style }

    /** Sets the focused block's style, or turns it back to body text. */
    fun toggleStyle(style: StyleArg) {
        val id = focused ?: return
        val cur = text(id)?.style ?: return
        editor.setStyle(id, if (cur == style) StyleArg.BODY else style)
        changed()
    }

    fun toggleChecked(block: String) {
        editor.toggleChecked(block)
        changed()
    }

    fun indent(delta: Int) {
        val id = focused ?: return
        editor.indent(id, delta)
        changed()
    }

    // ---- blocks -------------------------------------------------------------

    /** Where inserted blocks go: after the focused block, else at the end. */
    private fun anchor(): String? = focused ?: blocks.lastOrNull()?.id

    fun insertDivider() = typeAfter(editor.insertDivider(anchor()))

    fun insertImage(attachment: String, aspect: Float) = typeAfter(editor.insertImage(anchor(), attachment, aspect))

    /** After inserting [block], carries on typing on the line below it. */
    private fun typeAfter(block: String) {
        sync()
        val i = blocks.indexOfFirst { it.id == block }
        val next = blocks.getOrNull(i + 1)?.takeIf { it.kind is BlockKind.Text }
        changed(next?.let { CaretInfo(it.id, 0) })
    }

    fun moveBlock(block: String, delta: Int) {
        editor.moveBlock(block, delta)
        changed()
    }

    fun deleteBlock(block: String) {
        if (focused == block) focused = null
        changed(editor.deleteBlock(block))
    }

    fun setCaption(block: String, caption: String) {
        editor.setCaption(block, caption)
        changed()
    }

    // ---- ink ----------------------------------------------------------------

    /** Adds a finished stroke; [front] when the front buffer is showing it still. */
    fun addStroke(tool: ToolArg, pen: Pen, points: ByteArray, front: Boolean) {
        val path = editor.addStroke(tool, pen.color, pen.width, points, dark) ?: return
        val stroke = StrokePath(path)
        strokes = strokes + stroke
        if (front) frontTail++
        inkBottom = maxOf(inkBottom, stroke.bottom)
        inkChanged()
    }

    fun erase(x: Float, y: Float) {
        val radius = pens.getValue(InkTool.ERASER).width
        val gone = editor.erase(x, y, radius)
        if (gone.isEmpty()) return
        val at = gone.toHashSet()
        strokes = strokes.filterIndexed { i, _ -> i !in at }
        frontTail = 0
        inkChanged()
    }

    fun endGesture() = editor.endGesture()

    fun choosePaper(paper: PaperArg) {
        editor.setPaper(paper)
        changed()
    }

    fun clearInk() {
        editor.clearInk()
        changed()
    }

    // ---- history & saving ---------------------------------------------------

    fun undo() {
        pendingMarks = null
        changed(editor.undo())
        title = TextFieldValue(editor.title(), title.selection)
    }

    fun redo() {
        pendingMarks = null
        changed(editor.redo())
        title = TextFieldValue(editor.title(), title.selection)
    }

    /** Writes pending changes. */
    fun save() {
        if (editor.isDirty() && lib.save(editor)) refreshInfo()
    }

    // ---- note-level actions -------------------------------------------------

    fun setFavorite(on: Boolean) {
        lib.setFavorite(noteId, on)
        refreshInfo()
    }

    fun setPinned(on: Boolean) {
        lib.setPinned(noteId, on)
        refreshInfo()
    }

    fun setColor(color: String?) {
        lib.setNoteColor(noteId, color)
        refreshInfo()
    }

    fun updateDark(on: Boolean) {
        if (on == dark) return
        dark = on
        sync()
        reloadInk()
    }

    /** Stores picture bytes and inserts them; fails for non-pictures. */
    fun addPicture(bytes: ByteArray, name: String) {
        val image = lib.addImage(bytes, name)
        insertImage(image.attachment, image.aspect)
    }

    fun attachmentPath(id: String): String = lib.attachmentPath(id)

    /** The note as Markdown, saved first so it is current. */
    fun markdown(): String {
        save()
        return lib.markdownText(noteId)
    }

    fun trash() {
        save()
        lib.trashNote(noteId)
    }

    fun restore() = lib.restoreNote(noteId)

    fun deleteForever() = lib.deleteNote(noteId)

    fun moveTo(folder: String?) {
        lib.moveNote(noteId, folder)
        refreshInfo()
    }
}
