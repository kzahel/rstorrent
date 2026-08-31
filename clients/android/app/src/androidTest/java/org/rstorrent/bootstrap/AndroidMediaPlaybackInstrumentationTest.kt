package org.rstorrent.bootstrap

import android.content.ComponentName
import android.content.Context
import android.security.NetworkSecurityPolicy
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class AndroidMediaPlaybackInstrumentationTest {
    @Test
    fun cleartextPolicyAllowsOnlyExactLoopbackControl() {
        val policy = NetworkSecurityPolicy.getInstance()
        assertTrue(policy.isCleartextTrafficPermitted("127.0.0.1"))
        assertFalse(policy.isCleartextTrafficPermitted("localhost"))
        assertFalse(policy.isCleartextTrafficPermitted("example.com"))
    }

    @Test
    fun playerActivityIsPrivate() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val info =
            context.packageManager.getActivityInfo(
                ComponentName(context, PlayerActivity::class.java),
                0,
            )
        assertFalse(info.exported)
    }
}
