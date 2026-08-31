package org.rstorrent.bootstrap

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Build

/** Debug-APK ingress for physical product campaigns that must not disturb the UI task. */
class ProductTestReceiver : BroadcastReceiver() {
    override fun onReceive(
        context: Context,
        intent: Intent,
    ) {
        val serviceIntent =
            Intent(context, ProductEngineService::class.java).apply {
                action = ProductEngineService.ACTION_DEBUG_TORRENT_CONTROL
                replaceExtras(intent.extras)
            }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            context.startForegroundService(serviceIntent)
        } else {
            context.startService(serviceIntent)
        }
    }
}
