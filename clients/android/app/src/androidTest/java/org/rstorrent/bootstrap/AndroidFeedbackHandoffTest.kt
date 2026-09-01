package org.rstorrent.bootstrap

import android.content.ActivityNotFoundException
import android.content.Intent
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class AndroidFeedbackHandoffTest {
    private val environment = AndroidFeedbackEnvironment("0.1", "15", "Google Pixel 9")

    @Test
    fun createsOnePlainExternalActionViewIntent() {
        val started = mutableListOf<Intent>()
        var failures = 0

        val launched =
            AndroidFeedbackLauncher.launch(
                environment = environment,
                startExternalActivity = { started += Intent(it) },
                onFailure = { failures += 1 },
            )

        assertTrue(launched)
        assertEquals(0, failures)
        assertEquals(1, started.size)
        val intent = started.single()
        assertEquals(Intent.ACTION_VIEW, intent.action)
        assertEquals(
            "https://jstorrent.com/feedback.html" +
                "?platform=android&v=0.1&android=15&device=Google%20Pixel%209",
            intent.dataString,
        )
        assertNull(intent.component)
        assertNull(intent.`package`)
        assertNull(intent.selector)
        assertNull(intent.clipData)
        assertNull(intent.extras)
        assertNull(intent.type)
        assertEquals(0, intent.flags)
        assertTrue(intent.categories.isNullOrEmpty())
    }

    @Test
    fun missingHandlerAndLaunchRejectionAreVisibleAndNonfatal() {
        listOf<(Intent) -> Unit>(
            { throw ActivityNotFoundException("missing browser") },
            { throw SecurityException("launch rejected") },
        ).forEach { start ->
            var starts = 0
            var failures = 0

            val launched =
                AndroidFeedbackLauncher.launch(
                    environment = environment,
                    startExternalActivity = {
                        starts += 1
                        start(it)
                    },
                    onFailure = { failures += 1 },
                )

            assertFalse(launched)
            assertEquals(1, starts)
            assertEquals(1, failures)
        }
    }

    @Test
    fun rejectedOversizeUrlDoesNotStartAnActivity() {
        var starts = 0
        var failures = 0

        val launched =
            AndroidFeedbackLauncher.launch(
                environment = environment.copy(device = "a".repeat(AndroidFeedbackUrl.MAX_URL_BYTES)),
                startExternalActivity = { starts += 1 },
                onFailure = { failures += 1 },
            )

        assertFalse(launched)
        assertEquals(0, starts)
        assertEquals(1, failures)
    }
}
