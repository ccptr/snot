package app.snot

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.provider.OpenableColumns
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import app.snot.editor.EditorHost
import app.snot.editor.EditorScreen
import app.snot.editor.EditorState
import app.snot.editor.InkTool
import app.snot.ffi.FolderInfo
import app.snot.notes.LibraryState
import app.snot.notes.NotesHost
import app.snot.notes.NotesScreen
import app.snot.ui.SnotTheme

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val lib = (application as SnotApp).library
        setContent {
            SnotTheme { App(remember { LibraryState(lib) }) }
        }
    }

    fun share(title: String, markdown: String) {
        val send = Intent(Intent.ACTION_SEND).apply {
            type = "text/plain"
            putExtra(Intent.EXTRA_SUBJECT, title)
            putExtra(Intent.EXTRA_TEXT, markdown)
        }
        startActivity(Intent.createChooser(send, null))
    }
}

@Composable
private fun App(library: LibraryState) {
    val activity = androidx.activity.compose.LocalActivity.current as MainActivity
    var open by rememberSaveable { mutableStateOf<String?>(null) }
    // A note made by this visit to the editor, and whether it is for drawing.
    var fresh by rememberSaveable { mutableStateOf<String?>(null) }
    var freshInk by rememberSaveable { mutableStateOf(false) }
    // Bumped to reopen the same note afresh, after a restore.
    var generation by remember { mutableIntStateOf(0) }

    val importer = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri: Uri? ->
        if (uri == null) return@rememberLauncherForActivityResult
        try {
            val text = activity.contentResolver.openInputStream(uri)?.use { it.readBytes().decodeToString() }
                ?: return@rememberLauncherForActivityResult
            val name = activity.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
                ?.use { if (it.moveToFirst()) it.getString(0) else null }
                ?.substringBeforeLast('.')
                ?: "Imported note"
            open = library.importMarkdown(text, name)
        } catch (e: Exception) {
            Toast.makeText(activity, e.message ?: "Could not import that file.", Toast.LENGTH_LONG).show()
        }
    }

    val id = open
    if (id == null) {
        LifecycleEventEffect(Lifecycle.Event.ON_RESUME) { library.reload() }
        BackHandler(enabled = library.searching) { library.endSearch() }
        NotesScreen(library, object : NotesHost {
            override fun open(id: String) {
                open = id
            }

            override fun create(handwriting: Boolean) {
                val id = library.createNote()
                fresh = id
                freshInk = handwriting
                open = id
            }

            override fun importMarkdown() {
                importer.launch(arrayOf("text/markdown", "text/plain", "text/x-markdown", "application/octet-stream"))
            }
        })
        return
    }

    val dark = isSystemInDarkTheme()
    val state = remember(id, generation) {
        runCatching { EditorState(library.lib, library.lib.openNote(id), dark) }.getOrNull()?.also {
            if (fresh == id && freshInk) it.tool = InkTool.PEN
        }
    }
    if (state == null) {
        // The note is gone (deleted elsewhere); fall back to the list.
        open = null
        return
    }
    val close = {
        // Backing out of a new note without writing anything discards it.
        if (fresh == id && state.isBlank()) state.deleteForever() else state.save()
        fresh = null
        open = null
        library.reload()
    }
    LifecycleEventEffect(Lifecycle.Event.ON_STOP) { state.save() }
    BackHandler { close() }
    EditorScreen(state, object : EditorHost {
        override fun folders(): List<FolderInfo> = library.lib.folders()
        override fun close() = close()
        override fun reopen() {
            generation++
        }

        override fun share(title: String, markdown: String) = activity.share(title, markdown)
    })
}
