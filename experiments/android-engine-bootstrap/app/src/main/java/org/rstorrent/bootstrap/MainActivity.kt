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

        if (savedInstanceState == null) {
            val serviceIntent =
                Intent(this, EngineService::class.java).apply {
                    action = intent.action ?: BootstrapContract.ACTION_START
                    replaceExtras(intent.extras ?: Bundle())
                }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                startForegroundService(serviceIntent)
            } else {
                startService(serviceIntent)
            }
        }
        if (intent.getBooleanExtra("finish_activity", false)) {
            finish()
        }
    }
}
