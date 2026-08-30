package org.rstorrent.bootstrap

import android.content.Intent
import android.content.pm.ActivityInfo
import android.content.pm.PackageManager
import android.net.Uri
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.filters.SdkSuppress
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
@SdkSuppress(minSdkVersion = 33)
class ExternalIntentResolutionTest {
    private val context = ApplicationProvider.getApplicationContext<android.content.Context>()
    private val packageManager = context.packageManager
    private val targetPackage = context.packageName

    @Test
    fun mainActivityIsTheOnlyExportedSingleTopProductEntry() {
        val activity =
            packageManager.getActivityInfo(
                android.content.ComponentName(context, MainActivity::class.java),
                PackageManager.ComponentInfoFlags.of(0),
            )
        assertTrue(activity.exported)
        assertEquals(ActivityInfo.LAUNCH_SINGLE_TOP, activity.launchMode)

        listOf(EngineService::class.java, ProductEngineService::class.java).forEach { service ->
            val info =
                packageManager.getServiceInfo(
                    android.content.ComponentName(context, service),
                    PackageManager.ComponentInfoFlags.of(0),
                )
            assertFalse(info.exported)
        }
    }

    @Test
    fun manifestResolvesOnlyTheDeclaredExternalTorrentShapes() {
        val magnet = view(Uri.parse("magnet:?xt=urn:btih:${"a".repeat(40)}"))
        val exactMime =
            view(
                Uri.parse("content://fixture.invalid/items/opaque"),
                BITTORRENT_MIME_TYPE,
            )
        val suffix = view(Uri.parse("content://fixture.invalid/items/file.torrent"))

        listOf(magnet, exactMime, suffix).forEach { intent ->
            assertTrue(resolvesTarget(intent))
        }

        listOf(
            view(Uri.parse("file:///sdcard/Download/file.torrent")),
            view(Uri.parse("http://fixture.invalid/file.torrent")),
            view(Uri.parse("https://fixture.invalid/file.torrent")),
            view(
                Uri.parse("content://fixture.invalid/items/opaque"),
                "application/octet-stream",
            ),
            Intent(Intent.ACTION_SEND).apply {
                type = BITTORRENT_MIME_TYPE
                putExtra(Intent.EXTRA_STREAM, suffix.data)
            },
        ).forEach { intent ->
            assertFalse(resolvesTarget(intent))
        }
    }

    private fun view(
        uri: Uri,
        mimeType: String? = null,
    ): Intent =
        Intent(Intent.ACTION_VIEW).apply {
            addCategory(Intent.CATEGORY_BROWSABLE)
            if (mimeType == null) data = uri else setDataAndType(uri, mimeType)
        }

    private fun resolvesTarget(intent: Intent): Boolean =
        packageManager
            .queryIntentActivities(intent, PackageManager.ResolveInfoFlags.of(0))
            .any {
                it.activityInfo.packageName == targetPackage &&
                    it.activityInfo.name == MainActivity::class.java.name
            }
}
