package org.rstorrent.bootstrap

import java.net.URI
import java.net.URLDecoder
import java.nio.charset.StandardCharsets
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Test

class AndroidFeedbackUrlTest {
    @Test
    fun buildsTheExactStrictFourFieldUrl() {
        val environment =
            AndroidFeedbackEnvironment(
                applicationVersion = "0.1",
                androidRelease = "15",
                device = "Google Pixel 9",
            )

        val url = AndroidFeedbackUrl.build(environment)

        assertEquals(
            "https://jstorrent.com/feedback.html" +
                "?platform=android&v=0.1&android=15&device=Google%20Pixel%209",
            url,
        )
        val uri = URI(url)
        assertEquals("https", uri.scheme)
        assertEquals("jstorrent.com", uri.host)
        assertEquals(-1, uri.port)
        assertEquals("/feedback.html", uri.rawPath)
        assertEquals(null, uri.rawFragment)
        assertEquals(
            listOf("platform", "v", "android", "device"),
            queryFields(uri).map { it.first },
        )
    }

    @Test
    fun encodesSpacesReservedCharactersAndUnicodeWithoutChangingValues() {
        val environment =
            AndroidFeedbackEnvironment(
                applicationVersion = "1.0 beta&x",
                androidRelease = "15/preview",
                device = "Gřehl Pixel 端😀",
            )
        val original = environment.copy()

        val url = AndroidFeedbackUrl.build(environment)

        assertEquals(original, environment)
        assertEquals(
            "https://jstorrent.com/feedback.html" +
                "?platform=android&v=1.0%20beta%26x&android=15%2Fpreview" +
                "&device=G%C5%99ehl%20Pixel%20%E7%AB%AF%F0%9F%98%80",
            url,
        )
        assertEquals(
            listOf(
                "platform" to "android",
                "v" to environment.applicationVersion,
                "android" to environment.androidRelease,
                "device" to environment.device,
            ),
            queryFields(URI(url)),
        )
    }

    @Test
    fun closedKeySetCannotCarryProhibitedContext() {
        val url =
            AndroidFeedbackUrl.build(
                AndroidFeedbackEnvironment("version", "release", "manufacturer model"),
            )
        val keys = queryFields(URI(url)).mapTo(mutableSetOf()) { it.first }
        val prohibited =
            setOf(
                "installation",
                "profile",
                "report",
                "client",
                "days",
                "usage",
                "torrent",
                "hash",
                "magnet",
                "path",
                "root",
                "tracker",
                "peer",
                "endpoint",
                "settings",
                "logs",
                "error",
                "diagnostics",
                "lifecycle",
                "text",
                "token",
                "cookie",
                "credential",
            )

        assertEquals(setOf("platform", "v", "android", "device"), keys)
        assertFalse(keys.any(prohibited::contains))
    }

    @Test
    fun acceptsExactlyTwoKibibytesAndRejectsOneByteMore() {
        val empty = AndroidFeedbackEnvironment("", "", "")
        val fixedLength = AndroidFeedbackUrl.build(empty).length
        val exact = empty.copy(device = "a".repeat(AndroidFeedbackUrl.MAX_URL_BYTES - fixedLength))

        assertEquals(AndroidFeedbackUrl.MAX_URL_BYTES, AndroidFeedbackUrl.build(exact).length)
        assertThrows(IllegalArgumentException::class.java) {
            AndroidFeedbackUrl.build(exact.copy(device = exact.device + "a"))
        }
    }

    @Test
    fun rejectsMalformedUnicodeWithoutReplacement() {
        val malformed = String(charArrayOf('\uD800'))
        assertThrows(IllegalArgumentException::class.java) {
            AndroidFeedbackUrl.build(AndroidFeedbackEnvironment("0.1", "15", malformed))
        }
    }

    private fun queryFields(uri: URI): List<Pair<String, String>> =
        requireNotNull(uri.rawQuery).split('&').map { field ->
            val (key, value) = field.split('=', limit = 2)
            key to URLDecoder.decode(value, StandardCharsets.UTF_8)
        }
}
