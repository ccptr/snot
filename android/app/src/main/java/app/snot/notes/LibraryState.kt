package app.snot.notes

import androidx.compose.foundation.lazy.staggeredgrid.LazyStaggeredGridState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import app.snot.ffi.CountsInfo
import app.snot.ffi.FolderInfo
import app.snot.ffi.NoteInfo
import app.snot.ffi.ScopeArg
import app.snot.ffi.SnotLibrary
import app.snot.ffi.SortArg

/** The notes list: which notes are showing and how. Outlives the screen. */
class LibraryState(val lib: SnotLibrary) {
    var scope: ScopeArg by mutableStateOf(ScopeArg.All)
        private set
    var sort by mutableStateOf(SortArg.MODIFIED)
        private set
    var query by mutableStateOf("")
        private set
    var searching by mutableStateOf(false)
        private set

    var notes: List<NoteInfo> by mutableStateOf(emptyList())
        private set
    var folders: List<FolderInfo> by mutableStateOf(emptyList())
        private set
    var counts by mutableStateOf(CountsInfo(0, 0, 0, 0))
        private set

    val grid = LazyStaggeredGridState()

    init {
        reload()
    }

    fun reload() {
        folders = lib.folders()
        counts = lib.counts()
        // A folder that went away falls back to everything.
        val s = scope
        if (s is ScopeArg.Folder && folders.none { it.id == s.id }) scope = ScopeArg.All
        notes = if (searching && query.isNotBlank()) lib.search(query) else lib.notes(scope, sort)
    }

    fun show(scope: ScopeArg) {
        this.scope = scope
        searching = false
        query = ""
        reload()
    }

    fun sortBy(sort: SortArg) {
        this.sort = sort
        reload()
    }

    fun startSearch() {
        searching = true
    }

    fun search(q: String) {
        query = q
        reload()
    }

    fun endSearch() {
        searching = false
        query = ""
        reload()
    }

    val title: String
        get() = when (val s = scope) {
            ScopeArg.All -> "All notes"
            ScopeArg.Favorites -> "Favourites"
            ScopeArg.Unfiled -> "No folder"
            ScopeArg.Trash -> "Trash"
            is ScopeArg.Folder -> folders.firstOrNull { it.id == s.id }?.name ?: "Folder"
        }

    /** The folder new notes go into: the one showing, if any. */
    val newNoteFolder: String?
        get() = (scope as? ScopeArg.Folder)?.id

    fun createNote(): String = lib.createNote(newNoteFolder)

    private inline fun act(f: () -> Unit) {
        f()
        reload()
    }

    fun setPinned(n: NoteInfo, on: Boolean) = act { lib.setPinned(n.id, on) }
    fun setFavorite(n: NoteInfo, on: Boolean) = act { lib.setFavorite(n.id, on) }
    fun setColor(n: NoteInfo, color: String?) = act { lib.setNoteColor(n.id, color) }
    fun move(n: NoteInfo, folder: String?) = act { lib.moveNote(n.id, folder) }
    fun duplicate(n: NoteInfo) = act { lib.duplicateNote(n.id) }
    fun trash(n: NoteInfo) = act { lib.trashNote(n.id) }
    fun restore(n: NoteInfo) = act { lib.restoreNote(n.id) }
    fun delete(n: NoteInfo) = act { lib.deleteNote(n.id) }
    fun emptyTrash() = act { lib.emptyTrash() }
    fun createFolder(name: String) = act {
        val id = lib.createFolder(name, null)
        scope = ScopeArg.Folder(id)
    }
    fun renameFolder(id: String, name: String) = act { lib.renameFolder(id, name) }
    fun deleteFolder(id: String) = act { lib.deleteFolder(id) }
    fun importMarkdown(text: String, name: String): String {
        val id = lib.importMarkdownText(text, name, newNoteFolder)
        reload()
        return id
    }
}
