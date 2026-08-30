package org.rstorrent.bootstrap

import android.app.NotificationManager
import android.content.Context
import android.content.Intent
import android.os.IBinder
import android.os.SystemClock
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.rule.ServiceTestRule
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.RuleChain
import org.junit.rules.TestRule
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class ProductBackgroundLifecycleTest {
    private val permissionRule = NotificationPermissionRule()
    private val serviceRule = ServiceTestRule.withTimeout(30, TimeUnit.SECONDS)

    @get:Rule
    val rules: TestRule = RuleChain.outerRule(permissionRule).around(serviceRule)

    @Test
    fun visibleHandoffDemotesAndLatestIdleReleaseJoins() = runBlocking {
        val context = ApplicationProvider.getApplicationContext<Context>()
        assertTrue(ProductLifecyclePreferenceStore(context).write(ProductLifecyclePreferences()))
        ProductInteractionRegistry.resetForTest()
        ProductInteractionRegistry.setActivityVisible(true)
        context.startForegroundService(Intent(context, ProductEngineService::class.java))
        val binder: IBinder =
            serviceRule.bindService(Intent(context, ProductEngineService::class.java))
        val service = (binder as ProductEngineService.LocalBinder).service

        try {
            withTimeout(30_000L) {
                service.state.first { it.ready && !it.lifecycle.foreground }
            }
            assertFalse(service.state.value.lifecycle.backgroundDownloadsEnabled)
            assertFalse(service.resourceSnapshotForTest().foreground)
            waitForOngoingNotification(context, expected = false)

            service.setBackgroundDownloadsEnabled(true)
            assertTrue(service.state.value.lifecycle.backgroundDownloadsEnabled)
            assertTrue(service.state.value.lifecycle.effectiveBackgroundDownloads)

            ProductInteractionRegistry.setActivityVisible(false)
            withTimeout(5_000L) {
                service.state.first { it.lifecycle.foreground }
            }
            assertTrue(service.state.value.lifecycle.reason?.startsWith("wait_") == true)
            assertTrue(service.resourceSnapshotForTest().lifecycleDeadlineScheduled)
            assertFalse(service.resourceSnapshotForTest().backgroundAdmitted)

            ProductInteractionRegistry.setActivityVisible(true)
            withTimeout(5_000L) {
                service.state.first { !it.lifecycle.foreground }
            }
            assertFalse(service.resourceSnapshotForTest().shutdownComplete)

            ProductInteractionRegistry.setActivityVisible(false)
            val deadline = SystemClock.elapsedRealtime() + 10_000L
            while (
                !service.resourceSnapshotForTest().shutdownComplete &&
                    SystemClock.elapsedRealtime() < deadline
            ) {
                SystemClock.sleep(25L)
            }
            val terminal = service.resourceSnapshotForTest()
            assertTrue(terminal.shutdownComplete)
            assertFalse(terminal.foreground)
            assertFalse(terminal.backgroundAdmitted)
            assertFalse(terminal.notificationReceiverRegistered)
            assertFalse(terminal.networkCallbackRegistered)
            waitForOngoingNotification(context, expected = false)
        } finally {
            context.stopService(Intent(context, ProductEngineService::class.java))
            ProductInteractionRegistry.resetForTest()
            ProductLifecyclePreferenceStore(context).write(ProductLifecyclePreferences())
        }
    }

    private fun hasOngoingNotification(context: Context): Boolean =
        context
            .getSystemService(NotificationManager::class.java)
            .activeNotifications
            .any { it.id == AndroidNotificationContract.ONGOING_NOTIFICATION_ID }

    private fun waitForOngoingNotification(
        context: Context,
        expected: Boolean,
    ) {
        val deadline = SystemClock.elapsedRealtime() + 5_000L
        while (
            hasOngoingNotification(context) != expected &&
                SystemClock.elapsedRealtime() < deadline
        ) {
            SystemClock.sleep(25L)
        }
        if (expected) assertTrue(hasOngoingNotification(context))
        else assertFalse(hasOngoingNotification(context))
    }
}
