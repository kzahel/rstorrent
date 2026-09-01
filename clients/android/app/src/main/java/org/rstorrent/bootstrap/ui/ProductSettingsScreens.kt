package org.rstorrent.bootstrap.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import org.rstorrent.bootstrap.ProductNetworkState
import org.rstorrent.bootstrap.R
import org.rstorrent.session.uniffi.ActiveSeedLimit
import org.rstorrent.session.uniffi.ClientSettingsRuntimeView
import org.rstorrent.session.uniffi.EncryptionPolicy
import org.rstorrent.session.uniffi.ListenerPolicy
import org.rstorrent.session.uniffi.PortMappingPolicy
import org.rstorrent.session.uniffi.TransferRateLimit

@Composable
internal fun ConnectionLimitsSettings(
    settings: ClientSettingsRuntimeView,
    onPeerConnections: (UInt) -> Unit,
    onUploadSlots: (UShort) -> Unit,
    onActiveDownloads: (UShort) -> Unit,
    onActiveSeeds: (ActiveSeedLimit) -> Unit,
    onShareRatioLimit: (UInt) -> Unit,
    onFinishedDownloadRatioLimit: (UInt) -> Unit,
    onFinishedTimeLimit: (UInt) -> Unit,
    onUploadRateLimit: (TransferRateLimit) -> Unit,
    onDownloadRateLimit: (TransferRateLimit) -> Unit,
) {
    NumericSetting(
        stringResource(R.string.setting_peer_connections),
        settings.configured.peerConnectionLimit.toInt(),
        1..2_000,
        supporting =
            stringResource(
                R.string.setting_effective,
                settings.effectivePeerConnectionLimit.toLong(),
                settingsApplicationLabel(settings.peerConnectionsApplication),
            ),
    ) { onPeerConnections(it.toUInt()) }
    NumericSetting(
        stringResource(R.string.setting_upload_slots),
        settings.configured.uploadSlots.toInt(),
        0..50,
        supporting =
            stringResource(
                R.string.setting_effective,
                settings.effectiveUploadSlots.toLong(),
                settingsApplicationLabel(settings.uploadSlotsApplication),
            ),
    ) { onUploadSlots(it.toUShort()) }
    NumericSetting(
        stringResource(R.string.setting_active_downloads),
        settings.configured.activeDownloads.toInt(),
        1..20,
        supporting =
            settings.activeDownloadsClampReason?.let {
                stringResource(
                    R.string.setting_effective_android_clamped,
                    settings.effectiveActiveDownloads.toLong(),
                    it.name.lowercase(),
                )
            } ?: stringResource(
                R.string.setting_effective_android,
                settings.effectiveActiveDownloads.toLong(),
            ),
    ) { onActiveDownloads(it.toUShort()) }
    ActiveSeedSetting(
        configured = settings.configured.activeSeeds,
        effective = settings.effectiveActiveSeeds,
        activeCount = settings.activeSeedCount,
        inactiveCount = settings.inactiveSeedCount,
        onValue = onActiveSeeds,
    )
    NumericSetting(
        stringResource(R.string.setting_share_ratio_goal),
        settings.configured.shareRatioLimitPercent.toInt(),
        0..Int.MAX_VALUE,
        supporting =
            stringResource(R.string.setting_share_ratio_goal_detail),
    ) { onShareRatioLimit(it.toUInt()) }
    NumericSetting(
        stringResource(R.string.setting_finished_download_goal),
        settings.configured.finishedDownloadRatioLimitPercent.toInt(),
        0..Int.MAX_VALUE,
        supporting = stringResource(R.string.setting_finished_download_goal_detail),
    ) { onFinishedDownloadRatioLimit(it.toUInt()) }
    NumericSetting(
        stringResource(R.string.setting_finished_time_goal),
        settings.configured.finishedTimeLimitSeconds.toInt(),
        0..Int.MAX_VALUE,
        supporting = stringResource(R.string.setting_finished_time_goal_detail),
    ) { onFinishedTimeLimit(it.toUInt()) }
    RateLimitSetting(
        title = stringResource(R.string.setting_all_download_limit),
        configured = settings.configured.downloadRateLimit,
        effective = settings.effectiveDownloadRateLimit,
        application = settingsApplicationLabel(settings.bandwidthApplication),
        onValue = onDownloadRateLimit,
    )
    RateLimitSetting(
        title = stringResource(R.string.setting_all_upload_limit),
        configured = settings.configured.uploadRateLimit,
        effective = settings.effectiveUploadRateLimit,
        application = settingsApplicationLabel(settings.bandwidthApplication),
        onValue = onUploadRateLimit,
    )
}

@Composable
private fun ActiveSeedSetting(
    configured: ActiveSeedLimit,
    effective: ActiveSeedLimit,
    activeCount: UShort,
    inactiveCount: UShort,
    onValue: (ActiveSeedLimit) -> Unit,
) {
    var dialog by remember { mutableStateOf(false) }
    val configuredLabel = activeSeedLimitLabel(configured)
    ListItem(
        headlineContent = { Text(stringResource(R.string.setting_active_seeds)) },
        supportingContent = {
            Text(
                stringResource(
                    R.string.setting_active_seeds_detail,
                    activeSeedLimitLabel(effective),
                    activeCount.toLong(),
                    inactiveCount.toLong(),
                ),
            )
        },
        trailingContent = {
            Text(configuredLabel, color = MaterialTheme.colorScheme.primary)
        },
        modifier = Modifier.clickable { dialog = true },
    )
    HorizontalDivider()
    if (dialog) {
        var unlimited by remember(configured) {
            mutableStateOf(configured is ActiveSeedLimit.Unlimited)
        }
        var text by remember(configured) {
            mutableStateOf((configured as? ActiveSeedLimit.Limited)?.torrents?.toString() ?: "5")
        }
        val parsed = if (unlimited) null else text.toIntOrNull()?.takeIf { it in 0..500 }
        AlertDialog(
            onDismissRequest = { dialog = false },
            title = { Text(stringResource(R.string.setting_active_seeds)) },
            text = {
                Column(Modifier.fillMaxWidth()) {
                    ListItem(
                        headlineContent = { Text(stringResource(R.string.setting_unlimited)) },
                        supportingContent = { Text(stringResource(R.string.setting_active_seed_ceiling)) },
                        trailingContent = {
                            Switch(unlimited, onCheckedChange = { unlimited = it })
                        },
                    )
                    OutlinedTextField(
                        value = text,
                        onValueChange = { text = it.filter(Char::isDigit) },
                        enabled = !unlimited,
                        singleLine = true,
                        label = { Text(stringResource(R.string.setting_count)) },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                        supportingText = { Text(stringResource(R.string.setting_active_seed_range)) },
                        isError = !unlimited && parsed == null,
                    )
                }
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        onValue(
                            if (unlimited) ActiveSeedLimit.Unlimited
                            else ActiveSeedLimit.Limited(requireNotNull(parsed).toUShort()),
                        )
                        dialog = false
                    },
                    enabled = unlimited || parsed != null,
                ) { Text(stringResource(R.string.action_apply)) }
            },
            dismissButton = { TextButton(onClick = { dialog = false }) { Text(stringResource(R.string.action_cancel)) } },
        )
    }
}

@Composable
private fun activeSeedLimitLabel(limit: ActiveSeedLimit): String =
    when (limit) {
        ActiveSeedLimit.Unlimited -> stringResource(R.string.setting_unlimited)
        is ActiveSeedLimit.Limited -> limit.torrents.toString()
    }

@Composable
internal fun RateLimitSetting(
    title: String,
    configured: TransferRateLimit,
    effective: TransferRateLimit? = null,
    application: String? = null,
    onValue: (TransferRateLimit) -> Unit,
) {
    var dialog by remember { mutableStateOf(false) }
    val supporting =
        buildList {
            add(stringResource(R.string.setting_rate_summary, rateLimitLabel(configured, stringResource(R.string.setting_unlimited))))
            effective?.let {
                add(
                    stringResource(
                        R.string.setting_rate_effective,
                        rateLimitLabel(it, stringResource(R.string.setting_unlimited)),
                    ),
                )
            }
            application?.let(::add)
        }.joinToString(" · ")
    ListItem(
        headlineContent = { Text(title) },
        supportingContent = { Text(supporting) },
        trailingContent = {
            Text(
                rateLimitLabel(configured, stringResource(R.string.setting_unlimited)),
                color = MaterialTheme.colorScheme.primary,
            )
        },
        modifier = Modifier.clickable { dialog = true },
    )
    HorizontalDivider()
    if (dialog) {
        var unlimited by remember(configured) { mutableStateOf(configured is TransferRateLimit.Unlimited) }
        var text by remember(configured) {
            mutableStateOf(
                (configured as? TransferRateLimit.Limited)?.let {
                    rateLimitKiBValue(it.bytesPerSecond)
                } ?: "1024",
            )
        }
        val parsed = if (unlimited) TransferRateLimit.Unlimited else parseRateLimit(text)
        AlertDialog(
            onDismissRequest = { dialog = false },
            title = { Text(title) },
            text = {
                Column(Modifier.fillMaxWidth()) {
                    ListItem(
                        headlineContent = { Text(stringResource(R.string.setting_unlimited)) },
                        trailingContent = {
                            Switch(
                                checked = unlimited,
                                onCheckedChange = { unlimited = it },
                            )
                        },
                    )
                    OutlinedTextField(
                        value = text,
                        onValueChange = { text = it.filter { character -> character.isDigit() || character == '.' } },
                        enabled = !unlimited,
                        singleLine = true,
                        label = { Text(stringResource(R.string.setting_rate_unit)) },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                        supportingText = { Text(stringResource(R.string.setting_rate_precision)) },
                        isError = !unlimited && parsed == null,
                    )
                }
            },
            confirmButton = {
                TextButton(
                    onClick = { onValue(requireNotNull(parsed)); dialog = false },
                    enabled = parsed != null,
                ) { Text(stringResource(R.string.action_apply)) }
            },
            dismissButton = { TextButton(onClick = { dialog = false }) { Text(stringResource(R.string.action_cancel)) } },
        )
    }
}

internal fun parseRateLimit(valueKiB: String): TransferRateLimit.Limited? {
    if (!Regex("^\\d+(?:\\.\\d+)?$").matches(valueKiB)) return null
    val bytes = (valueKiB.toDoubleOrNull() ?: return null) * 1024.0
    if (!bytes.isFinite() || bytes % 1.0 != 0.0 || bytes < 1_024.0 || bytes > UInt.MAX_VALUE.toDouble()) {
        return null
    }
    return TransferRateLimit.Limited(bytes.toLong().toUInt())
}

internal fun rateLimitLabel(
    limit: TransferRateLimit,
    unlimitedLabel: String,
): String =
    when (limit) {
        TransferRateLimit.Unlimited -> unlimitedLabel
        is TransferRateLimit.Limited -> "${rateLimitKiBValue(limit.bytesPerSecond)} KiB/s"
    }

private fun rateLimitKiBValue(bytesPerSecond: UInt): String {
    val kib = bytesPerSecond.toDouble() / 1024.0
    return if (kib % 1.0 == 0.0) kib.toLong().toString() else kib.toString()
}

@Composable
internal fun NetworkSettings(
    settings: ClientSettingsRuntimeView,
    network: ProductNetworkState,
    onUnmeteredNetworksOnly: (Boolean) -> Unit,
    onListener: (Boolean) -> Unit,
    onPortMapping: (Boolean) -> Unit,
    onIpv6: (Boolean) -> Unit,
    onDht: (Boolean) -> Unit,
    onPeerExchange: (Boolean) -> Unit,
    onEncryption: (EncryptionPolicy) -> Unit,
) {
    ToggleSetting(
        title = stringResource(R.string.setting_unmetered_only),
        detail =
            stringResource(
                R.string.setting_unmetered_detail,
                productNetworkTruthText(network.currentTruth),
                settings.applicationNetwork.state.name.lowercase(),
            ),
        checked = network.unmeteredNetworksOnly,
        onChecked = onUnmeteredNetworksOnly,
    )
    network.preferenceError?.let {
        ListItem(
            headlineContent = { Text(stringResource(R.string.setting_not_saved)) },
            supportingContent = { Text(productErrorText(it)) },
        )
        HorizontalDivider()
    }
    network.runtimeError?.let {
        ListItem(
            headlineContent = { Text(stringResource(R.string.setting_network_attention)) },
            supportingContent = { Text(productErrorText(it)) },
        )
        HorizontalDivider()
    }
    ToggleSetting(
        title = stringResource(R.string.setting_incoming_connections),
        detail = settings.listenerStatus.toString(),
        checked = settings.configured.listener !is ListenerPolicy.Disabled,
        onChecked = onListener,
    )
    ToggleSetting(
        title = stringResource(R.string.setting_upnp),
        detail = settings.portMappingStatus.toString(),
        checked = settings.configured.portMapping == PortMappingPolicy.UPNP,
        onChecked = onPortMapping,
    )
    ToggleSetting(
        title = stringResource(R.string.setting_ipv6),
        detail = settingsApplicationLabel(settings.ipv6Application),
        checked = settings.configured.ipv6Enabled,
        onChecked = onIpv6,
    )
    ToggleSetting(
        title = stringResource(R.string.setting_dht),
        detail =
            stringResource(
                R.string.setting_dht_detail,
                settingsApplicationLabel(settings.dhtApplication),
            ),
        checked = settings.configured.dhtEnabled,
        onChecked = onDht,
    )
    ToggleSetting(
        title = stringResource(R.string.setting_peer_exchange),
        detail =
            stringResource(
                R.string.setting_peer_exchange_detail,
                settingsApplicationLabel(settings.peerExchangeApplication),
            ),
        checked = settings.configured.peerExchangeEnabled,
        onChecked = onPeerExchange,
    )
    ChoiceSetting(
        title = stringResource(R.string.setting_peer_encryption),
        selected = settings.configured.encryption,
        values = EncryptionPolicy.entries,
        label = { encryptionLabel(it) },
        detail =
            stringResource(
                R.string.setting_effective_value,
                encryptionLabel(settings.effectiveEncryption),
                settingsApplicationLabel(settings.encryptionApplication),
            ),
        onSelected = onEncryption,
    )
    DisabledSetting(stringResource(R.string.setting_vpn_only))
    DisabledSetting(stringResource(R.string.setting_proxy))
}

@Composable
private fun encryptionLabel(policy: EncryptionPolicy): String =
    stringResource(
        when (policy) {
            EncryptionPolicy.DISABLED -> R.string.encryption_disabled
            EncryptionPolicy.ALLOW -> R.string.encryption_allow
            EncryptionPolicy.PREFER -> R.string.encryption_prefer
            EncryptionPolicy.REQUIRED -> R.string.encryption_required
        },
    )

@Composable
private fun NumericSetting(
    title: String,
    value: Int,
    range: IntRange,
    supporting: String? = null,
    onValue: (Int) -> Unit,
) {
    var dialog by remember { mutableStateOf(false) }
    ListItem(
        headlineContent = { Text(title) },
        supportingContent = {
            Text(
                supporting
                    ?: stringResource(R.string.setting_allowed_range, range.first, range.last),
            )
        },
        trailingContent = { Text(value.toString(), color = MaterialTheme.colorScheme.primary) },
        modifier = Modifier.clickable { dialog = true },
    )
    HorizontalDivider()
    if (dialog) {
        var text by remember(value) { mutableStateOf(value.toString()) }
        val parsed = text.toIntOrNull()
        AlertDialog(
            onDismissRequest = { dialog = false },
            title = { Text(title) },
            text = {
                OutlinedTextField(
                    value = text,
                    onValueChange = { text = it.filter(Char::isDigit) },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    supportingText = {
                        Text(stringResource(R.string.setting_range, range.first, range.last))
                    },
                )
            },
            confirmButton = {
                TextButton(
                    onClick = { onValue(requireNotNull(parsed)); dialog = false },
                    enabled = parsed != null && parsed in range,
                ) { Text(stringResource(R.string.action_apply)) }
            },
            dismissButton = { TextButton(onClick = { dialog = false }) { Text(stringResource(R.string.action_cancel)) } },
        )
    }
}

@Composable
private fun ToggleSetting(
    title: String,
    detail: String,
    checked: Boolean,
    onChecked: (Boolean) -> Unit,
) {
    ListItem(
        headlineContent = { Text(title) },
        supportingContent = { Text(detail) },
        trailingContent = {
            Switch(
                checked = checked,
                onCheckedChange = onChecked,
                modifier = Modifier.semantics { contentDescription = title },
            )
        },
    )
    HorizontalDivider()
}

@Composable
private fun <T> ChoiceSetting(
    title: String,
    selected: T,
    values: List<T>,
    label: @Composable (T) -> String,
    detail: String? = null,
    onSelected: (T) -> Unit,
) {
    var dialog by remember { mutableStateOf(false) }
    ListItem(
        headlineContent = { Text(title) },
        supportingContent = { Text(listOfNotNull(label(selected), detail).joinToString(" · ")) },
        modifier = Modifier.clickable { dialog = true },
    )
    HorizontalDivider()
    if (dialog) {
        AlertDialog(
            onDismissRequest = { dialog = false },
            title = { Text(title) },
            text = {
                Column(Modifier.fillMaxWidth()) {
                    values.forEach { value ->
                        Text(
                            label(value),
                            modifier =
                                Modifier.fillMaxWidth().clickable {
                                    onSelected(value)
                                    dialog = false
                                }.padding(16.dp),
                            color =
                                if (value == selected) MaterialTheme.colorScheme.primary
                                else MaterialTheme.colorScheme.onSurface,
                        )
                    }
                }
            },
            confirmButton = { TextButton(onClick = { dialog = false }) { Text(stringResource(R.string.action_cancel)) } },
        )
    }
}

@Composable
internal fun ReadOnlySettingsRow(
    title: String,
    detail: String,
) {
    ListItem(headlineContent = { Text(title) }, supportingContent = { Text(detail) })
    HorizontalDivider()
}

@Composable
internal fun DisabledSetting(title: String) {
    ListItem(
        headlineContent = { Text(title) },
        supportingContent = { Text(stringResource(R.string.setting_not_available)) },
        colors =
            androidx.compose.material3.ListItemDefaults.colors(
                headlineColor = MaterialTheme.colorScheme.onSurfaceVariant,
                supportingColor = MaterialTheme.colorScheme.outline,
            ),
    )
    HorizontalDivider()
}
