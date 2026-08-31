@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package org.rstorrent.bootstrap.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material3.Card
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import org.rstorrent.bootstrap.R
import org.rstorrent.session.uniffi.DhtInspectionView
import org.rstorrent.session.uniffi.SessionCurrentRatesView
import org.rstorrent.session.uniffi.SpeedHistoryView
import org.rstorrent.session.uniffi.SpeedSeriesView

@Composable
internal fun SpeedScreen(
    history: SpeedHistoryView?,
    currentRates: SessionCurrentRatesView?,
    onBack: () -> Unit,
) {
    ProductRouteScaffold(stringResource(R.string.speed_title), onBack) {
        if (history == null) {
            item("loading") { RouteMessage(stringResource(R.string.speed_loading)) }
        } else {
            item("summary") {
                Column(Modifier.fillMaxWidth().padding(16.dp)) {
                    Text(stringResource(R.string.speed_history_title), fontWeight = FontWeight.SemiBold)
                    Text(
                        stringResource(
                            R.string.speed_history_summary,
                            history.range.name.lowercase(),
                            history.bucketMillis.toLong(),
                            stringResource(
                                if (history.live) {
                                    R.string.speed_history_live
                                } else {
                                    R.string.speed_history_snapshot
                                },
                            ),
                        ),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            val rates = currentRates?.rates?.associate { it.metric to it.bytes }.orEmpty()
            items(history.series, key = { it.metric.name }) { series ->
                SpeedSeriesCard(series, rates[series.metric])
            }
            item("scope") {
                RouteMessage(
                    stringResource(R.string.speed_history_scope),
                )
            }
        }
    }
}

@Composable
private fun SpeedSeriesCard(
    series: SpeedSeriesView,
    currentRate: String?,
) {
    val values = series.values.map { it?.toULongOrNull()?.toFloat() ?: 0f }
    Card(Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 6.dp)) {
        Column(Modifier.padding(12.dp)) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                Text(series.metric.name.lowercase().replace('_', ' '), fontWeight = FontWeight.Medium)
                Text(formatRate(currentRate))
            }
            Spacer(Modifier.height(8.dp))
            Sparkline(values, Modifier.fillMaxWidth().height(72.dp), MaterialTheme.colorScheme.primary)
        }
    }
}

@Composable
private fun Sparkline(
    values: List<Float>,
    modifier: Modifier,
    color: Color,
) {
    Canvas(modifier) {
        if (values.size < 2) return@Canvas
        val maximum = values.maxOrNull()?.takeIf { it > 0f } ?: 1f
        val path = Path()
        values.forEachIndexed { index, value ->
            val x = size.width * index / (values.size - 1).coerceAtLeast(1)
            val y = size.height - (value / maximum * size.height)
            if (index == 0) path.moveTo(x, y) else path.lineTo(x, y)
        }
        drawLine(
            color.copy(alpha = 0.2f),
            Offset(0f, size.height),
            Offset(size.width, size.height),
            strokeWidth = 1.dp.toPx(),
        )
        drawPath(path, color, style = androidx.compose.ui.graphics.drawscope.Stroke(2.dp.toPx()))
    }
}

@Composable
internal fun DhtScreen(
    inspection: DhtInspectionView?,
    onBack: () -> Unit,
) {
    ProductRouteScaffold(stringResource(R.string.library_menu_dht), onBack) {
        if (inspection == null) {
            item("loading") { RouteMessage(stringResource(R.string.dht_loading)) }
        } else {
            item("summary") {
                Column(Modifier.fillMaxWidth().padding(16.dp)) {
                    Text(
                        stringResource(
                            R.string.dht_state_summary,
                            inspection.lifecycle.name.lowercase(),
                            inspection.networkPolicy.name.lowercase(),
                        ),
                        style = MaterialTheme.typography.titleMedium,
                    )
                    Text(
                        stringResource(
                            R.string.dht_summary,
                            inspection.activeTransactions.toLong(),
                            inspection.activeLookups.toLong(),
                            inspection.discoveredPeers.toLong(),
                        ),
                    )
                }
                HorizontalDivider()
            }
            items(inspection.families, key = { it.family.name }) { family ->
                ListItem(
                    headlineContent = { Text(family.family.name) },
                    supportingContent = {
                        Column {
                            Text(
                                stringResource(
                                    R.string.dht_family_summary,
                                    family.lifecycle.name.lowercase(),
                                    family.routingNodes.toLong(),
                                    family.occupiedBuckets.toLong(),
                                ),
                            )
                            Text(
                                family.observedExternalAddress ?: family.localAddress,
                                fontFamily = FontFamily.Monospace,
                            )
                        }
                    },
                )
                HorizontalDivider()
            }
            item("traffic") {
                ListItem(
                    headlineContent = { Text(stringResource(R.string.dht_traffic)) },
                    supportingContent = {
                        Text(
                            stringResource(
                                R.string.dht_traffic_summary,
                                formatBytes(inspection.datagramBytesSent),
                                formatBytes(inspection.datagramBytesReceived),
                                inspection.malformedReceived.toLong(),
                                inspection.rateLimited.toLong(),
                            ),
                        )
                    },
                )
            }
        }
    }
}

@Composable
private fun ProductRouteScaffold(
    title: String,
    onBack: () -> Unit,
    content: androidx.compose.foundation.lazy.LazyListScope.() -> Unit,
) {
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(title) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(
                            Icons.AutoMirrored.Outlined.ArrowBack,
                            contentDescription = stringResource(R.string.action_back),
                        )
                    }
                },
            )
        },
    ) { padding -> LazyColumn(Modifier.fillMaxSize().padding(padding), content = content) }
}

@Composable
private fun RouteMessage(message: String) {
    Box(Modifier.fillMaxWidth().padding(24.dp)) {
        Text(message, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}
