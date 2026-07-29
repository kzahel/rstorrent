package org.rstorrent.bootstrap

import android.app.Activity
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.widget.TextView

class MainActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(
            TextView(this).apply {
                text = "RSTorrent engine bootstrap"
                textSize = 18f
                setPadding(32, 32, 32, 32)
            },
        )

        dispatch(intent)
        if (intent.getBooleanExtra("finish_activity", false)) {
            finish()
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        dispatch(intent)
        if (intent.getBooleanExtra("finish_activity", false)) {
            finish()
        }
    }

    private fun dispatch(command: Intent) {
        val serviceIntent =
            Intent(this, EngineService::class.java).apply {
                action = command.action ?: BootstrapContract.ACTION_START
                replaceExtras(command.extras ?: Bundle())
            }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            startForegroundService(serviceIntent)
        } else {
            startService(serviceIntent)
        }
    }
}
