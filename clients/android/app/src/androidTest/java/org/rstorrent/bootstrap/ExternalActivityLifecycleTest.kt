package org.rstorrent.bootstrap

import android.content.Intent
import android.net.Uri
import androidx.test.core.app.ActivityScenario
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.rule.ServiceTestRule
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class ExternalActivityLifecycleTest {
    @get:Rule val serviceRule = ServiceTestRule.withTimeout(15, TimeUnit.SECONDS)

    private val context = ApplicationProvider.getApplicationContext<android.content.Context>()

    @Test
    fun coldWarmAndRecreatedActivityConsumeEachDeliveryOnce() {
        val first = magnet('1')
        val second = magnet('2')
        ActivityScenario.launch<MainActivity>(externalView(first)).use { scenario ->
            val service = bindProductService()
            awaitDepth(service, 1)
            assertSanitized(scenario)

            scenario.recreate()
            assertSanitized(scenario)
            assertEquals(1, service.state.value.externalIntakeDepth)

            context.startActivity(
                externalView(second).addFlags(
                    Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP,
                ),
            )
            awaitDepth(service, 2)
            assertSanitized(scenario)

            while (service.state.value.externalIntakeDepth > 0) {
                val intakeId = requireNotNull(service.state.value.externalIntake?.intakeId)
                service.cancelExternalIntake(intakeId)
                awaitDepth(service, service.state.value.externalIntakeDepth - 1)
            }
        }
        context.stopService(Intent(context, ProductEngineService::class.java))
    }

    private fun bindProductService(): ProductEngineService {
        val binder =
            serviceRule.bindService(
                Intent(context, ProductEngineService::class.java),
            ) as ProductEngineService.LocalBinder
        return binder.service
    }

    private fun awaitDepth(
        service: ProductEngineService,
        depth: Int,
    ) = runBlocking {
        withTimeout(5_000L) {
            service.state.first { it.externalIntakeDepth == depth }
        }
    }

    private fun assertSanitized(scenario: ActivityScenario<MainActivity>) {
        scenario.onActivity { activity ->
            assertEquals(Intent.ACTION_MAIN, activity.intent.action)
            assertNull(activity.intent.data)
        }
    }

    private fun externalView(source: String): Intent =
        Intent(context, MainActivity::class.java).apply {
            action = Intent.ACTION_VIEW
            data = Uri.parse(source)
        }

    private fun magnet(digit: Char): String =
        "magnet:?xt=urn:btih:${digit.toString().repeat(40)}"
}
