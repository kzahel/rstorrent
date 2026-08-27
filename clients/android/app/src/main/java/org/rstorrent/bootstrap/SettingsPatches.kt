package org.rstorrent.bootstrap

import org.rstorrent.session.uniffi.ClientSettings
import org.rstorrent.session.uniffi.ClientSettingsPatch
import org.rstorrent.session.uniffi.EncryptionPolicy
import org.rstorrent.session.uniffi.HttpsServerAuthenticationPolicy
import org.rstorrent.session.uniffi.ListenerPolicy
import org.rstorrent.session.uniffi.PortMappingPolicy
import org.rstorrent.session.uniffi.TorrentSettingsPatch
import org.rstorrent.session.uniffi.TorrentTransferLimits
import org.rstorrent.session.uniffi.TransferRateLimit

internal fun clientSettingsPatch(
    listener: ListenerPolicy? = null,
    preferredListenPort: UShort? = null,
    portMapping: PortMappingPolicy? = null,
    peerConnectionLimit: UInt? = null,
    uploadSlots: UShort? = null,
    activeDownloads: UShort? = null,
    uploadRateLimit: TransferRateLimit? = null,
    downloadRateLimit: TransferRateLimit? = null,
    encryption: EncryptionPolicy? = null,
    ipv6Enabled: Boolean? = null,
    trackerHttpsServerAuthentication: HttpsServerAuthenticationPolicy? = null,
): ClientSettingsPatch =
    ClientSettingsPatch(
        listener,
        preferredListenPort,
        portMapping,
        peerConnectionLimit,
        uploadSlots,
        activeDownloads,
        uploadRateLimit,
        downloadRateLimit,
        encryption,
        ipv6Enabled,
        trackerHttpsServerAuthentication,
    )

internal fun ClientSettings.asPatch(): ClientSettingsPatch =
    clientSettingsPatch(
        listener = listener,
        preferredListenPort = preferredListenPort,
        portMapping = portMapping,
        peerConnectionLimit = peerConnectionLimit,
        uploadSlots = uploadSlots,
        activeDownloads = activeDownloads,
        uploadRateLimit = uploadRateLimit,
        downloadRateLimit = downloadRateLimit,
        encryption = encryption,
        ipv6Enabled = ipv6Enabled,
        trackerHttpsServerAuthentication = trackerHttpsServerAuthentication,
    )

internal fun torrentSettingsPatch(
    uploadRateLimit: TransferRateLimit? = null,
    downloadRateLimit: TransferRateLimit? = null,
): TorrentSettingsPatch = TorrentSettingsPatch(uploadRateLimit, downloadRateLimit)

internal fun TorrentTransferLimits.asPatch(): TorrentSettingsPatch =
    torrentSettingsPatch(upload, download)
