package org.rstorrent.bootstrap

import android.content.ActivityNotFoundException
import android.content.Intent
import android.net.Uri
import java.net.URI
import java.net.URISyntaxException

internal data class AndroidFeedbackEnvironment(
    val applicationVersion: String,
    val androidRelease: String,
    val device: String,
)

internal object AndroidFeedbackUrl {
    const val BASE_URL = "https://jstorrent.com/feedback.html"
    const val MAX_URL_BYTES = 2 * 1_024

    private val baseUri =
        URI(BASE_URL).also { uri ->
            require(uri.scheme == "https")
            require(uri.host == "jstorrent.com")
            require(uri.port == -1)
            require(uri.userInfo == null)
            require(uri.rawPath == "/feedback.html")
            require(uri.rawQuery == null)
            require(uri.rawFragment == null)
        }

    fun build(environment: AndroidFeedbackEnvironment): String {
        val result = StringBuilder(BASE_URL)
        appendQueryValue(result, "?platform=", "android")
        appendQueryValue(result, "&v=", environment.applicationVersion)
        appendQueryValue(result, "&android=", environment.androidRelease)
        appendQueryValue(result, "&device=", environment.device)
        require(result.length <= MAX_URL_BYTES) { "feedback URL exceeds the 2 KiB limit" }

        val url = result.toString()
        val parsed =
            try {
                URI(url)
            } catch (error: URISyntaxException) {
                throw IllegalArgumentException("feedback URL is not a valid URI", error)
            }
        require(parsed.scheme == baseUri.scheme)
        require(parsed.host == baseUri.host)
        require(parsed.port == baseUri.port)
        require(parsed.userInfo == null)
        require(parsed.rawPath == baseUri.rawPath)
        require(parsed.rawFragment == null)
        return url
    }

    fun validateReviewed(url: String): String {
        require(url.length <= MAX_URL_BYTES) { "feedback URL exceeds the 2 KiB limit" }
        val parsed =
            try {
                URI(url)
            } catch (error: URISyntaxException) {
                throw IllegalArgumentException("feedback URL is not a valid URI", error)
            }
        require(parsed.scheme == baseUri.scheme)
        require(parsed.host == baseUri.host)
        require(parsed.port == baseUri.port)
        require(parsed.userInfo == null)
        require(parsed.rawPath == baseUri.rawPath)
        require(parsed.rawFragment == null)
        return url
    }

    private fun appendQueryValue(
        output: StringBuilder,
        prefix: String,
        value: String,
    ) {
        appendBounded(output, prefix)
        var index = 0
        while (index < value.length) {
            val first = value[index]
            val codePoint =
                when {
                    first.isHighSurrogate() -> {
                        require(index + 1 < value.length && value[index + 1].isLowSurrogate()) {
                            "feedback value contains invalid Unicode"
                        }
                        val next = value[index + 1]
                        index += 2
                        Character.toCodePoint(first, next)
                    }
                    first.isLowSurrogate() ->
                        throw IllegalArgumentException("feedback value contains invalid Unicode")
                    else -> {
                        index += 1
                        first.code
                    }
                }
            if (isUnreserved(codePoint)) {
                appendBounded(output, codePoint.toChar())
            } else {
                appendUtf8(output, codePoint)
            }
        }
    }

    private fun isUnreserved(codePoint: Int): Boolean =
        codePoint in 'a'.code..'z'.code ||
            codePoint in 'A'.code..'Z'.code ||
            codePoint in '0'.code..'9'.code ||
            codePoint == '-'.code ||
            codePoint == '.'.code ||
            codePoint == '_'.code ||
            codePoint == '~'.code

    private fun appendUtf8(
        output: StringBuilder,
        codePoint: Int,
    ) {
        when {
            codePoint <= 0x7f -> appendPercentEncoded(output, codePoint)
            codePoint <= 0x7ff -> {
                appendPercentEncoded(output, 0xc0 or (codePoint shr 6))
                appendPercentEncoded(output, 0x80 or (codePoint and 0x3f))
            }
            codePoint <= 0xffff -> {
                appendPercentEncoded(output, 0xe0 or (codePoint shr 12))
                appendPercentEncoded(output, 0x80 or ((codePoint shr 6) and 0x3f))
                appendPercentEncoded(output, 0x80 or (codePoint and 0x3f))
            }
            else -> {
                appendPercentEncoded(output, 0xf0 or (codePoint shr 18))
                appendPercentEncoded(output, 0x80 or ((codePoint shr 12) and 0x3f))
                appendPercentEncoded(output, 0x80 or ((codePoint shr 6) and 0x3f))
                appendPercentEncoded(output, 0x80 or (codePoint and 0x3f))
            }
        }
    }

    private fun appendPercentEncoded(
        output: StringBuilder,
        byte: Int,
    ) {
        appendBounded(output, '%')
        appendBounded(output, HEX[byte shr 4 and 0x0f])
        appendBounded(output, HEX[byte and 0x0f])
    }

    private fun appendBounded(
        output: StringBuilder,
        value: String,
    ) {
        require(output.length + value.length <= MAX_URL_BYTES) {
            "feedback URL exceeds the 2 KiB limit"
        }
        output.append(value)
    }

    private fun appendBounded(
        output: StringBuilder,
        value: Char,
    ) {
        require(output.length < MAX_URL_BYTES) { "feedback URL exceeds the 2 KiB limit" }
        output.append(value)
    }

    private const val HEX = "0123456789ABCDEF"
}

internal object AndroidFeedbackLauncher {
    fun launchReviewed(
        url: String,
        startExternalActivity: (Intent) -> Unit,
        onFailure: () -> Unit,
    ): Boolean {
        val intent =
            try {
                Intent(Intent.ACTION_VIEW, Uri.parse(AndroidFeedbackUrl.validateReviewed(url)))
            } catch (_: IllegalArgumentException) {
                onFailure()
                return false
            }
        return launchIntent(intent, startExternalActivity, onFailure)
    }

    fun launch(
        environment: AndroidFeedbackEnvironment,
        startExternalActivity: (Intent) -> Unit,
        onFailure: () -> Unit,
    ): Boolean {
        val intent =
            try {
                Intent(Intent.ACTION_VIEW, Uri.parse(AndroidFeedbackUrl.build(environment)))
            } catch (_: IllegalArgumentException) {
                onFailure()
                return false
            }

        return launchIntent(intent, startExternalActivity, onFailure)
    }

    private fun launchIntent(
        intent: Intent,
        startExternalActivity: (Intent) -> Unit,
        onFailure: () -> Unit,
    ): Boolean = try {
            startExternalActivity(intent)
            true
        } catch (_: ActivityNotFoundException) {
            onFailure()
            false
        } catch (_: SecurityException) {
            onFailure()
            false
        }
}
