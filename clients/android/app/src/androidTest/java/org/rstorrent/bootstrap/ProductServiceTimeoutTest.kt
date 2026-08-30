package org.rstorrent.bootstrap

import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.IBinder
import android.os.SystemClock
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.rule.ServiceTestRule
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.RuleChain
import org.junit.rules.TestRule
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class ProductServiceTimeoutTest {
    private val permissionRule =
        NotificationPermissionRule()
    private val serviceRule = ServiceTestRule()

    @get:Rule
    val rules: TestRule = RuleChain.outerRule(permissionRule).around(serviceRule)

    @Test
    fun dataSyncTimeoutUsesJoinedNonStickyShutdown() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        ProductInteractionRegistry.resetForTest()
        context.startForegroundService(Intent(context, ProductEngineService::class.java))
        val binder: IBinder =
            serviceRule.bindService(Intent(context, ProductEngineService::class.java))
        val service = (binder as ProductEngineService.LocalBinder).service

        service.onTimeout(1, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        val deadline = SystemClock.elapsedRealtime() + 30_000L
        var snapshot = service.resourceSnapshotForTest()
        while (!snapshot.shutdownComplete && SystemClock.elapsedRealtime() < deadline) {
            SystemClock.sleep(25L)
            snapshot = service.resourceSnapshotForTest()
        }

        assertTrue(snapshot.shutdownComplete)
        assertEquals(0, snapshot.interactionLeases)
        assertFalse(snapshot.notificationReceiverRegistered)
        assertFalse(snapshot.wakeLockHeld)
        assertFalse(
            context
                .getSystemService(android.app.NotificationManager::class.java)
                .activeNotifications
                .any { it.id == AndroidNotificationContract.ONGOING_NOTIFICATION_ID },
        )
        context.stopService(Intent(context, ProductEngineService::class.java))
        ProductInteractionRegistry.resetForTest()
    }
}
