# JNA looks its classes up by name, and UniFFI's generated structures are
# read reflectively.
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-keep class app.snot.ffi.** { *; }
-dontwarn java.awt.**
