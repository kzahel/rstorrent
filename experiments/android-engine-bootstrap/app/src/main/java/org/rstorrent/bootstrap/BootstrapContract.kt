package org.rstorrent.bootstrap

object BootstrapContract {
    const val ACTION_START = "org.rstorrent.bootstrap.START"
    const val ACTION_CANCEL = "org.rstorrent.bootstrap.CANCEL"
    const val ACTION_OBSERVE = "org.rstorrent.bootstrap.OBSERVE"
    const val EXPECTED_INTERFACE = "rstorrent-android/0.1.0;uniffi/0.31.0"
    const val MAX_METAINFO_BASE64_CHARS = 256 * 1024
    const val MAX_RUN_ID_CHARS = 64

    private val runIdPattern = Regex("[A-Za-z0-9][A-Za-z0-9._-]{0,63}")

    fun requireRunId(value: String?): String {
        require(value != null && runIdPattern.matches(value)) {
            "run_id must match ${runIdPattern.pattern}"
        }
        return value
    }
}
