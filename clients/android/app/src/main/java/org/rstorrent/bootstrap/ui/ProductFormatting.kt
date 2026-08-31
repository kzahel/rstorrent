package org.rstorrent.bootstrap.ui

import java.math.BigInteger
import java.util.Locale
import kotlin.math.max
import org.rstorrent.session.uniffi.ClientSettingsApplicationState
import org.rstorrent.session.uniffi.SeedAdmissionView
import org.rstorrent.session.uniffi.SeedGoalView
import org.rstorrent.session.uniffi.TorrentEtaView
import org.rstorrent.session.uniffi.TorrentOperationalState
import org.rstorrent.session.uniffi.TorrentView

internal fun formatBytes(decimal: String?): String {
    val bytes = decimal?.toDoubleOrNull() ?: return "—"
    val units = arrayOf("B", "KiB", "MiB", "GiB", "TiB")
    var value = bytes
    var unit = 0
    while (value >= 1024.0 && unit < units.lastIndex) {
        value /= 1024.0
        unit += 1
    }
    val pattern = if (unit == 0) "%.0f %s" else "%.1f %s"
    return String.format(Locale.getDefault(), pattern, value, units[unit])
}

internal fun formatRate(decimal: String?): String =
    if ((decimal?.toULongOrNull() ?: 0UL) == 0UL) "0 B/s" else "${formatBytes(decimal)}/s"

internal fun torrentPresentationName(torrent: TorrentView): String =
    torrent.displayName ?: torrent.sourceDisplayName ?: torrent.torrentId

internal fun formatDuration(seconds: ULong): String {
    val days = seconds / 86_400UL
    val hours = seconds % 86_400UL / 3_600UL
    val minutes = seconds % 3_600UL / 60UL
    val remaining = seconds % 60UL
    return when {
        days > 0UL -> "${days}d ${hours}h"
        hours > 0UL -> "${hours}h ${minutes}m"
        minutes > 0UL -> "${minutes}m ${remaining}s"
        else -> "${remaining}s"
    }
}

internal fun formatDuration(decimal: String): String =
    decimal.toULongOrNull()?.let(::formatDuration) ?: "—"

internal fun formatShareRatio(hundredths: String?): String {
    val value = hundredths?.toBigIntegerOrNull()?.takeIf { it.signum() >= 0 } ?: return "—"
    val parts = value.divideAndRemainder(BigInteger.valueOf(100))
    return "${parts[0]}.${parts[1].toString().padStart(2, '0')}"
}

internal fun seedAdmissionLabel(admission: SeedAdmissionView): String =
    when (admission) {
        SeedAdmissionView.ACTIVE -> "Active seed"
        SeedAdmissionView.QUEUED -> "Queued seed"
        SeedAdmissionView.INACTIVE_EXEMPT -> "Active, inactive-exempt seed"
        SeedAdmissionView.INELIGIBLE -> "Not eligible to seed"
    }

internal fun seedGoalLabel(goal: SeedGoalView?): String {
    goal ?: return "—"
    val thresholds =
        buildList {
            if (goal.shareRatioMet) add("share ratio")
            if (goal.finishedDownloadRatioMet) add("finished/download time ratio")
            if (goal.finishedTimeMet) add("finished time")
        }.ifEmpty { listOf("none yet") }
    return "${goal.status.name.lowercase()} · met: ${thresholds.joinToString()}"
}

internal fun torrentProgress(torrent: TorrentView): Float {
    val required = torrent.requiredPayloadBytes?.toDoubleOrNull()
    val remaining = torrent.remainingPayloadBytes?.toDoubleOrNull()
    if (required != null && remaining != null && required > 0.0) {
        return ((required - remaining) / required).coerceIn(0.0, 1.0).toFloat()
    }
    if (torrent.pieceCount > 0U) {
        return (torrent.verifiedPieceCount.toFloat() / torrent.pieceCount.toFloat())
            .coerceIn(0f, 1f)
    }
    return 0f
}

internal fun torrentEta(torrent: TorrentView): String =
    when (val eta = torrent.eta) {
        is TorrentEtaView.Estimate -> eta.seconds.toULongOrNull()?.let(::formatDuration) ?: "—"
        TorrentEtaView.WarmingUp -> "Calculating…"
        TorrentEtaView.Stalled -> "Stalled"
        TorrentEtaView.Unavailable -> "—"
    }

internal fun operationalLabel(state: TorrentOperationalState): String =
    state.name.lowercase().replaceFirstChar(Char::titlecase)

internal fun settingsApplicationLabel(state: ClientSettingsApplicationState): String =
    when (state) {
        ClientSettingsApplicationState.Applied -> "Applied"
        ClientSettingsApplicationState.Applying -> "Applying…"
        is ClientSettingsApplicationState.Degraded -> state.detail
    }

internal fun percentLabel(progress: Float): String =
    String.format(Locale.getDefault(), "%.1f%%", max(0f, progress) * 100f)
