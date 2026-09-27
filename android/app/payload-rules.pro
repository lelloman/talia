# AndroidViewModelFactory constructs this ViewModel through reflection.
-keepclassmembers class com.lelloman.talia.NativeConnection {
    public <init>(android.app.Application);
}

# Initializer names are stored in the installed shell's manifest metadata.
-keep class * implements androidx.startup.Initializer { *; }
