package org.rstorrent.bootstrap

import android.app.Activity
import android.annotation.SuppressLint
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.widget.TextView

class MainActivity : Activity() {
    private var pendingCommand: Intent? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(
            TextView(this).apply {
                text = "RSTorrent engine bootstrap"
                textSize = 18f
                setPadding(32, 32, 32, 32)
            },
        )

        handle(intent)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handle(intent)
    }

    @SuppressLint("WrongConstant")
    override fun onActivityResult(
        requestCode: Int,
        resultCode: Int,
        data: Intent?,
    ) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != TREE_REQUEST) {
            return
        }
        val command = pendingCommand ?: error("SAF command was not retained")
        val treeUri = data?.data
        if (resultCode != RESULT_OK || treeUri == null) {
            finish()
            return
        }
        val flags =
            data.flags and (
                Intent.FLAG_GRANT_READ_URI_PERMISSION or
                    Intent.FLAG_GRANT_WRITE_URI_PERMISSION
            )
        contentResolver.takePersistableUriPermission(treeUri, flags)
        command.putExtra("tree_uri", treeUri.toString())
        dispatch(command)
        pendingCommand = null
        finishIfRequested(command)
    }

    private fun handle(command: Intent) {
        val storage = command.getStringExtra("storage") ?: "private"
        if (
            (command.action ?: BootstrapContract.ACTION_START) ==
                BootstrapContract.ACTION_START &&
            storage.startsWith("saf-") &&
            command.getStringExtra("tree_uri") == null
        ) {
            pendingCommand = Intent(command)
            val picker =
                Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).apply {
                    addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                    addFlags(Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
                    addFlags(Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION)
                    addFlags(Intent.FLAG_GRANT_PREFIX_URI_PERMISSION)
                    val initial =
                        command.getStringExtra("tree_initial_uri")
                            ?: "content://com.android.externalstorage.documents/document/primary%3ADownload"
                    putExtra(
                        "android.provider.extra.INITIAL_URI",
                        android.net.Uri.parse(initial),
                    )
                }
            startActivityForResult(picker, TREE_REQUEST)
            return
        }
        dispatch(command)
        finishIfRequested(command)
    }

    private fun finishIfRequested(command: Intent) {
        if (command.getBooleanExtra("finish_activity", false)) {
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

    companion object {
        private const val TREE_REQUEST = 51
    }
}
