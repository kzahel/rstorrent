package org.rstorrent.bootstrap

enum class TorrentPresentation {
    SUMMARY,
    FILES,
    TRACKERS,
    PEERS,
    PIECES,
}

enum class GlobalPresentation {
    NONE,
    SPEED,
    DHT,
}
