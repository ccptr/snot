package app.snot

import android.app.Application
import app.snot.ffi.SnotLibrary

/** Opens the note library once per process; every screen shares it. */
class SnotApp : Application() {
    val library: SnotLibrary by lazy { SnotLibrary.open(filesDir.resolve("snot").path) }
}
