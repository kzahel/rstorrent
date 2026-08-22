package org.rstorrent.bootstrap

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Build

class CommandReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val serviceIntent =
            Intent(context, EngineService::class.java).apply {
                action = intent.action
                replaceExtras(intent.extras)
            }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            context.startForegroundService(serviceIntent)
        } else {
            context.startService(serviceIntent)
        }
    }
}
