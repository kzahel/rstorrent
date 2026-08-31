package org.rstorrent.bootstrap

import java.net.URI
import java.util.concurrent.atomic.AtomicBoolean
import org.rstorrent.session.uniffi.FileView
import org.rstorrent.session.uniffi.MediaFileAvailability

internal object AndroidMediaPlaybackPolicy {
    private val videoExtensions =
        setOf(
            "mp4",
            "mkv",
            "avi",
            "webm",
            "mov",
            "m4v",
            "ts",
            "mts",
            "m2ts",
            "flv",
            "wmv",
            "ogv",
            "3gp",
        )
    private val capabilityPath = Regex("^/media/v1/[A-Za-z0-9_-]{43}$")

    fun isRecognizedVideo(path: List<String>): Boolean {
        val name = path.lastOrNull() ?: return false
        val separator = name.lastIndexOf('.')
        if (separator < 0 || separator == name.lastIndex) return false
        return name.substring(separator + 1).lowercase() in videoExtensions
    }

    fun isPlayable(
        path: List<String>,
        padding: Boolean,
        availability: MediaFileAvailability,
    ): Boolean =
        !padding &&
            isRecognizedVideo(path) &&
            availability in
            setOf(
                MediaFileAvailability.AVAILABLE,
                MediaFileAvailability.STREAMABLE,
            )

    fun isPlayActionEnabled(
        file: FileView,
        launchPending: Boolean,
    ): Boolean =
        !launchPending &&
            isPlayable(file.path, file.padding, file.mediaAvailability)

    fun requireCapabilityUrl(source: String): String {
        val uri =
            runCatching { URI(source) }
                .getOrElse { throw IllegalArgumentException("Invalid media source") }
        require(!uri.isOpaque) { "Invalid media source" }
        require(uri.scheme == "http") { "Media source must use HTTP loopback" }
        require(uri.host == "127.0.0.1") { "Media source must use IPv4 loopback" }
        require(uri.port in 1..65_535) { "Media source must use an explicit port" }
        require(uri.rawUserInfo == null) { "Media source must not contain user information" }
        require(uri.rawQuery == null) { "Media source must not contain a query" }
        require(uri.rawFragment == null) { "Media source must not contain a fragment" }
        require(capabilityPath.matches(uri.rawPath.orEmpty())) {
            "Media source must contain an exact capability path"
        }
        return source
    }

    fun unavailableMessage(reason: MediaFileAvailability): String =
        when (reason) {
            MediaFileAvailability.INCOMPLETE -> "This file does not have a playable verified range yet"
            MediaFileAvailability.CHECKING -> "This file is still being checked"
            MediaFileAvailability.UNVERIFIED -> "This file has not been verified yet"
            MediaFileAvailability.STORAGE_UNAVAILABLE -> "The download folder is unavailable"
            MediaFileAvailability.REMOVING -> "This torrent is being removed"
            MediaFileAvailability.RESOURCE_LIMIT -> "Too many media streams are active"
            MediaFileAvailability.PADDING,
            MediaFileAvailability.INVALID_FILE,
            MediaFileAvailability.METADATA_UNAVAILABLE,
            -> "This file is not available for playback"
            MediaFileAvailability.SERVER_UNAVAILABLE -> "The media server is unavailable"
            MediaFileAvailability.AVAILABLE,
            MediaFileAvailability.STREAMABLE,
            -> "Playback could not be started"
        }
}

internal class MediaLaunchGate {
    private val pending = AtomicBoolean(false)

    fun tryAcquire(): Boolean = pending.compareAndSet(false, true)

    fun release() {
        check(pending.compareAndSet(true, false)) { "media launch gate was not held" }
    }

    fun isPending(): Boolean = pending.get()
}
