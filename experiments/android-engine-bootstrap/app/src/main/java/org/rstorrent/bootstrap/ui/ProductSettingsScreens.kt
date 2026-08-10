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
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import org.rstorrent.session.uniffi.ClientSettingsRuntimeView
import org.rstorrent.session.uniffi.EncryptionPolicy
import org.rstorrent.session.uniffi.ListenerPolicy
import org.rstorrent.session.uniffi.PortMappingPolicy

@Composable
internal fun ConnectionLimitsSettings(
    settings: ClientSettingsRuntimeView,
    onPeerConnections: (UInt) -> Unit,
    onUploadSlots: (UShort) -> Unit,
    onActiveDownloads: (UShort) -> Unit,
) {
    NumericSetting(
        "Peer connections",
        settings.configured.peerConnectionLimit.toInt(),
        1..2_000,
        supporting =
            "Effective ${settings.effectivePeerConnectionLimit} · " +
                settingsApplicationLabel(settings.peerConnectionsApplication),
    ) { onPeerConnections(it.toUInt()) }
    NumericSetting(
        "Upload slots",
        settings.configured.uploadSlots.toInt(),
        0..50,
        supporting =
            "Effective ${settings.effectiveUploadSlots} · " +
                settingsApplicationLabel(settings.uploadSlotsApplication),
    ) { onUploadSlots(it.toUShort()) }
    NumericSetting(
        "Active downloads",
        settings.configured.activeDownloads.toInt(),
        1..20,
        supporting =
            "Effective on Android: ${settings.effectiveActiveDownloads}" +
                (settings.activeDownloadsClampReason?.let { " · ${it.name.lowercase()}" } ?: ""),
    ) { onActiveDownloads(it.toUShort()) }
    DisabledSetting("Download rate limit")
    DisabledSetting("Upload rate limit")
}

@Composable
internal fun NetworkSettings(
    settings: ClientSettingsRuntimeView,
    onListener: (Boolean) -> Unit,
    onPortMapping: (Boolean) -> Unit,
    onIpv6: (Boolean) -> Unit,
    onEncryption: (EncryptionPolicy) -> Unit,
) {
    ToggleSetting(
        title = "Incoming connections",
        detail = settings.listenerStatus.toString(),
        checked = settings.configured.listener !is ListenerPolicy.Disabled,
        onChecked = onListener,
    )
    ToggleSetting(
        title = "UPnP port mapping",
        detail = settings.portMappingStatus.toString(),
        checked = settings.configured.portMapping == PortMappingPolicy.UPNP,
        onChecked = onPortMapping,
    )
    ToggleSetting(
        title = "IPv6",
        detail = settingsApplicationLabel(settings.ipv6Application),
        checked = settings.configured.ipv6Enabled,
        onChecked = onIpv6,
    )
    ChoiceSetting(
        title = "Peer encryption",
        selected = settings.configured.encryption,
        values = EncryptionPolicy.entries,
        label = { it.name.lowercase().replaceFirstChar(Char::titlecase) },
        detail =
            "Effective ${settings.effectiveEncryption.name.lowercase()} · " +
                settingsApplicationLabel(settings.encryptionApplication),
        onSelected = onEncryption,
    )
    DisabledSetting("VPN-only mode")
    DisabledSetting("Metered network policy")
    DisabledSetting("Proxy")
}

@Composable
private fun NumericSetting(
    title: String,
    value: Int,
    range: IntRange,
    supporting: String = "Allowed ${range.first}–${range.last}",
    onValue: (Int) -> Unit,
) {
    var dialog by remember { mutableStateOf(false) }
    ListItem(
        headlineContent = { Text(title) },
        supportingContent = { Text(supporting) },
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
                    supportingText = { Text("${range.first}–${range.last}") },
                )
            },
            confirmButton = {
                TextButton(
                    onClick = { onValue(requireNotNull(parsed)); dialog = false },
                    enabled = parsed != null && parsed in range,
                ) { Text("Apply") }
            },
            dismissButton = { TextButton(onClick = { dialog = false }) { Text("Cancel") } },
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
        trailingContent = { Switch(checked, onCheckedChange = onChecked) },
    )
    HorizontalDivider()
}

@Composable
private fun <T> ChoiceSetting(
    title: String,
    selected: T,
    values: List<T>,
    label: (T) -> String,
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
            confirmButton = { TextButton(onClick = { dialog = false }) { Text("Cancel") } },
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
        supportingContent = { Text("Not available yet") },
        colors =
            androidx.compose.material3.ListItemDefaults.colors(
                headlineColor = MaterialTheme.colorScheme.onSurfaceVariant,
                supportingColor = MaterialTheme.colorScheme.outline,
            ),
    )
    HorizontalDivider()
}
