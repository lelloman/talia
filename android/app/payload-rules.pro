# AndroidViewModelFactory constructs this ViewModel through reflection.
-keepclassmembers class com.lelloman.talia.NativeConnection {
    public <init>(android.app.Application);
}

# Initializer names are stored in the installed shell's manifest metadata.
-keep class * implements androidx.startup.Initializer { *; }

# Paravoid's payload R8 selects R8-targeted coroutines rules but does not rewrite
# META-INF/services, so ServiceLoader still needs these implementations by name.
-keep class kotlinx.coroutines.android.AndroidDispatcherFactory { <init>(); }
-keep class kotlinx.coroutines.android.AndroidExceptionPreHandler { <init>(); }
