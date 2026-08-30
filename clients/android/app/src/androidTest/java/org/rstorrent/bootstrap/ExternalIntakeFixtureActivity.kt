package org.rstorrent.bootstrap

import android.app.Activity
import android.content.ComponentName
import android.content.Intent
import android.net.Uri
import android.os.Bundle

class ExternalIntakeFixtureActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val targetPackage = requireNotNull(intent.getStringExtra(EXTRA_TARGET_PACKAGE))
        val target = ComponentName(targetPackage, MainActivity::class.java.name)
        val magnet = intent.getStringExtra(EXTRA_MAGNET)
        val external =
            if (magnet != null) {
                Intent(Intent.ACTION_VIEW, Uri.parse(magnet), this, MainActivity::class.java)
                    .setComponent(target)
            } else {
                val fixture = intent.getStringExtra(EXTRA_FIXTURE) ?: ExternalIntakeFixtureProvider.VALID
                val uri = Uri.parse("content://$packageName.external-intake-fixture/$fixture")
                intent.getStringExtra(EXTRA_PAYLOAD_BASE64)?.let { encoded ->
                    contentResolver.call(
                        uri,
                        "configure",
                        null,
                        Bundle().apply { putString("payload_base64", encoded) },
                    )
                }
                grantUriPermission(targetPackage, uri, Intent.FLAG_GRANT_READ_URI_PERMISSION)
                Intent(Intent.ACTION_VIEW).apply {
                    component = target
                    setDataAndType(uri, intent.getStringExtra(EXTRA_MIME_TYPE))
                    addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                }
            }
        startActivity(external)
        finish()
    }

    companion object {
        const val EXTRA_TARGET_PACKAGE = "target_package"
        const val EXTRA_FIXTURE = "fixture"
        const val EXTRA_MIME_TYPE = "mime_type"
        const val EXTRA_MAGNET = "magnet"
        const val EXTRA_PAYLOAD_BASE64 = "payload_base64"
    }
}
