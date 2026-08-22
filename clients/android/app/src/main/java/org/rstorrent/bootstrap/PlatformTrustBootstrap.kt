package org.rstorrent.bootstrap

import android.app.Application
import android.content.Context

internal class OnceProcessInitializer {
    @Volatile
    private var initialized = false

    @Synchronized
    fun run(initializer: () -> Unit) {
        if (initialized) return
        initializer()
        initialized = true
    }
}

object PlatformTrustBootstrap {
    private val once = OnceProcessInitializer()

    init {
        System.loadLibrary("rstorrent_android")
    }

    fun ensureInitialized(context: Context) {
        once.run {
            initializeNative(context.applicationContext)
        }
    }

    @JvmStatic
    private external fun initializeNative(context: Context)
}

class RstorrentApplication : Application() {
    override fun onCreate() {
        super.onCreate()
        PlatformTrustBootstrap.ensureInitialized(this)
    }
}
