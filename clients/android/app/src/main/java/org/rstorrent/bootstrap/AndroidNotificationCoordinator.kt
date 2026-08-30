package org.rstorrent.bootstrap

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.util.Log
import androidx.core.app.NotificationCompat
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.update
import org.rstorrent.session.uniffi.TorrentState
import org.rstorrent.session.uniffi.ViewPatch
import org.rstorrent.session.uniffi.ViewSnapshot
import org.rstorrent.session.uniffi.ViewUpdate
import org.rstorrent.session.uniffi.ViewUpdatePayload

internal object AndroidNotificationContract {
    const val BACKGROUND_CHANNEL_ID = "rstorrent-product"
    const val COMPLETION_CHANNEL_ID = "rstorrent-downloads-completed"
    const val ATTENTION_CHANNEL_ID = "rstorrent-action-required"
    const val ONGOING_NOTIFICATION_ID = 42
    const val COMPANION_ROOT_NOTIFICATION_ID = 43
    const val EXTRA_TORRENT_ID = "notification_torrent_id"
    const val EXTRA_ROUTE = "notification_route"
    const val EXTRA_STORAGE_ROOT_ID = "notification_storage_root_id"
    const val ROUTE_TORRENT = "torrent"
    const val ROUTE_STORAGE_REPAIR = "storage_repair"

    fun routeAction(
        packageName: String,
        opaqueTag: String,
    ): String = "$packageName.action.NOTIFICATION_ROUTE.$opaqueTag"
}

internal class ProductNotificationPreferenceStore(context: Context) {
    private val preferences =
        context.getSharedPreferences(PREFERENCE_FILE, Context.MODE_PRIVATE)

    fun read(): ProductNotificationPreferences =
        ProductNotificationPreferences(
            downloadComplete = preferences.getBoolean(KEY_DOWNLOAD_COMPLETE, true),
            needsAttention = preferences.getBoolean(KEY_NEEDS_ATTENTION, true),
        )

    fun write(
        preference: ProductNotificationPreference,
        enabled: Boolean,
    ): Boolean =
        preferences
            .edit()
            .putBoolean(
                when (preference) {
                    ProductNotificationPreference.DOWNLOAD_COMPLETE -> KEY_DOWNLOAD_COMPLETE
                    ProductNotificationPreference.NEEDS_ATTENTION -> KEY_NEEDS_ATTENTION
                },
                enabled,
            ).commit()

    companion object {
        private const val PREFERENCE_FILE = "product_notifications"
        private const val KEY_DOWNLOAD_COMPLETE = "notify_download_complete"
        private const val KEY_NEEDS_ATTENTION = "notify_needs_attention"
    }
}

/** Owns Android notification channels, delivery, and the pure torrent-edge policy. */
internal class AndroidNotificationCoordinator(
    private val context: Context,
    private val state: MutableStateFlow<ProductState>,
    private val manager: NotificationManager =
        context.getSystemService(NotificationManager::class.java),
    private val preferenceStore: ProductNotificationPreferenceStore =
        ProductNotificationPreferenceStore(context),
) {
    private val policy = AndroidNotificationPolicy()
    private var preferences = preferenceStore.read()

    fun initialize(interactionLeaseCount: Int) {
        createChannels()
        refreshPlatformState(interactionLeaseCount)
    }

    fun onTorrentListUpdate(
        update: ViewUpdate,
        product: ProductState,
    ) {
        when (val payload = update.payload) {
            is ViewUpdatePayload.Snapshot -> {
                val snapshot = payload.snapshot as? ViewSnapshot.TorrentList ?: return
                policy.baseline(snapshot.torrents)
            }
            is ViewUpdatePayload.Patch -> {
                val patch = payload.patch as? ViewPatch.TorrentList ?: return
                val affected =
                    buildSet {
                        patch.upsert.mapTo(this) { it.torrentId }
                        patch.updates.mapTo(this) { it.torrentId }
                    }
                val rows = affected.mapNotNull(product.torrents::get)
                val reduction = policy.applyPatch(rows, patch.removed)
                reduction.removedTorrentIds.forEach(::cancelTorrentNotifications)
                reduction.edges.forEach(::deliver)
            }
            is ViewUpdatePayload.ResetRequired -> policy.reset()
        }
    }

    fun onTorrentListReset() {
        policy.reset()
    }

    fun setPreference(
        preference: ProductNotificationPreference,
        enabled: Boolean,
        interactionLeaseCount: Int,
    ) {
        when (
            val result =
                persistNotificationPreference(
                    preferences,
                    preference,
                    enabled,
                    preferenceStore::write,
                )
        ) {
            is ProductNotificationPreferenceResult.Applied -> {
                preferences = result.preferences
                state.update {
                    it.copy(
                        notifications =
                            platformState(interactionLeaseCount).copy(preferenceError = null),
                    )
                }
            }
            ProductNotificationPreferenceResult.Failed -> {
                state.update {
                    it.copy(
                        notifications =
                            it.notifications.copy(
                                preferenceError = "Notification setting could not be saved.",
                            ),
                    )
                }
            }
        }
    }

    fun refreshPlatformState(interactionLeaseCount: Int): NotificationEligibility {
        val notifications = platformState(interactionLeaseCount)
        state.update { it.copy(notifications = notifications) }
        return notifications.eligibility
    }

    fun ongoingNotification(detail: String): Notification {
        val open =
            PendingIntent.getActivity(
                context,
                ONGOING_OPEN_REQUEST,
                Intent(context, MainActivity::class.java).setPackage(context.packageName),
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
        val stop =
            PendingIntent.getService(
                context,
                ONGOING_STOP_REQUEST,
                Intent(context, ProductEngineService::class.java)
                    .setPackage(context.packageName)
                    .setAction(ProductEngineService.ACTION_STOP),
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
        return NotificationCompat
            .Builder(context, AndroidNotificationContract.BACKGROUND_CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_rstorrent_notification)
            .setContentTitle("RSTorrent")
            .setContentText(detail)
            .setContentIntent(open)
            .setOngoing(true)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
            .addAction(
                android.R.drawable.ic_menu_close_clear_cancel,
                "Stop",
                stop,
            ).build()
    }

    fun updateOngoingNotification(detail: String) {
        runCatching {
            manager.notify(
                AndroidNotificationContract.ONGOING_NOTIFICATION_ID,
                ongoingNotification(detail),
            )
        }.onFailure { error ->
            Log.w(TAG, "notification_delivery category=background result=rejected", error)
        }
    }

    fun cancelCompanionRootNotification() {
        manager.cancel(AndroidNotificationContract.COMPANION_ROOT_NOTIFICATION_ID)
    }

    fun showCompanionRootNotification(pendingIntent: PendingIntent): Boolean {
        val platform = platformState(state.value.notifications.interactionLeaseCount)
        state.update { it.copy(notifications = platform) }
        if (
            !platform.permissionGranted ||
            !platform.appNotificationsEnabled ||
            !platform.attentionChannelEnabled
        ) {
            return false
        }
        val notification =
            NotificationCompat
                .Builder(context, AndroidNotificationContract.ATTENTION_CHANNEL_ID)
                .setSmallIcon(R.drawable.ic_rstorrent_notification)
                .setContentTitle("Choose an RSTorrent download folder")
                .setContentText("Chrome needs Android folder access to continue")
                .setContentIntent(pendingIntent)
                .setAutoCancel(true)
                .setCategory(NotificationCompat.CATEGORY_ERROR)
                .setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
                .build()
        return runCatching {
            manager.notify(AndroidNotificationContract.COMPANION_ROOT_NOTIFICATION_ID, notification)
            true
        }.getOrElse { error ->
            Log.w(TAG, "notification_delivery category=workflow result=rejected", error)
            false
        }
    }

    fun close() {
        policy.reset()
        cancelCompanionRootNotification()
    }

    private fun createChannels() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val background =
            NotificationChannel(
                AndroidNotificationContract.BACKGROUND_CHANNEL_ID,
                "Background activity",
                NotificationManager.IMPORTANCE_LOW,
            ).apply {
                description = "Shows when RSTorrent is running in the background"
                setShowBadge(false)
                setSound(null, null)
                enableVibration(false)
            }
        val completion =
            NotificationChannel(
                AndroidNotificationContract.COMPLETION_CHANNEL_ID,
                "Downloads completed",
                NotificationManager.IMPORTANCE_DEFAULT,
            ).apply {
                description = "Alerts when a download genuinely finishes"
            }
        val attention =
            NotificationChannel(
                AndroidNotificationContract.ATTENTION_CHANNEL_ID,
                "Action required",
                NotificationManager.IMPORTANCE_HIGH,
            ).apply {
                description = "Alerts when a torrent or download folder needs attention"
            }
        manager.createNotificationChannels(listOf(background, completion, attention))
    }

    private fun deliver(edge: ProductNotificationEdge) {
        val platform = platformState(state.value.notifications.interactionLeaseCount)
        state.update { it.copy(notifications = platform) }
        if (!preferences.enabled(edge.category)) return
        if (!platform.permissionGranted || !platform.appNotificationsEnabled) return
        val channelEnabled =
            when (edge.category) {
                ProductNotificationCategory.DOWNLOAD_COMPLETE ->
                    platform.completionChannelEnabled
                ProductNotificationCategory.NEEDS_ATTENTION -> platform.attentionChannelEnabled
            }
        if (!channelEnabled || !makeRoom(edge.category)) return

        val tag = productNotificationTag(edge.category, edge.torrentId)
        val intent =
            Intent(context, MainActivity::class.java).apply {
                action = AndroidNotificationContract.routeAction(context.packageName, tag)
                setPackage(context.packageName)
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
                putExtra(AndroidNotificationContract.EXTRA_TORRENT_ID, edge.torrentId)
                when (val route = edge.route) {
                    ProductNotificationRoute.Torrent ->
                        putExtra(
                            AndroidNotificationContract.EXTRA_ROUTE,
                            AndroidNotificationContract.ROUTE_TORRENT,
                        )
                    is ProductNotificationRoute.StorageRepair -> {
                        putExtra(
                            AndroidNotificationContract.EXTRA_ROUTE,
                            AndroidNotificationContract.ROUTE_STORAGE_REPAIR,
                        )
                        putExtra(AndroidNotificationContract.EXTRA_STORAGE_ROOT_ID, route.rootId)
                    }
                }
            }
        val pending =
            PendingIntent.getActivity(
                context,
                tag.hashCode(),
                intent,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
        val channel =
            when (edge.category) {
                ProductNotificationCategory.DOWNLOAD_COMPLETE ->
                    AndroidNotificationContract.COMPLETION_CHANNEL_ID
                ProductNotificationCategory.NEEDS_ATTENTION ->
                    AndroidNotificationContract.ATTENTION_CHANNEL_ID
            }
        val (title, body) =
            when (edge.category) {
                ProductNotificationCategory.DOWNLOAD_COMPLETE ->
                    "Download complete" to "${edge.displayName} finished downloading"
                ProductNotificationCategory.NEEDS_ATTENTION ->
                    "Download needs attention" to
                        "${edge.displayName} · Open RSTorrent for details"
            }
        val notification =
            NotificationCompat
                .Builder(context, channel)
                .setSmallIcon(R.drawable.ic_rstorrent_notification)
                .setContentTitle(title)
                .setContentText(body)
                .setContentIntent(pending)
                .setAutoCancel(true)
                .setCategory(
                    if (edge.category == ProductNotificationCategory.DOWNLOAD_COMPLETE) {
                        NotificationCompat.CATEGORY_STATUS
                    } else {
                        NotificationCompat.CATEGORY_ERROR
                    },
                ).setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
                .build()
        runCatching { manager.notify(tag, eventNotificationId(edge.category), notification) }
            .onFailure { error ->
                Log.w(
                    TAG,
                    "notification_delivery category=${edge.category.name.lowercase()} " +
                        "result=rejected",
                    error,
                )
                refreshPlatformState(platform.interactionLeaseCount)
            }
    }

    private fun makeRoom(category: ProductNotificationCategory): Boolean {
        val id = eventNotificationId(category)
        val prefix = productNotificationTagPrefix(category)
        val active =
            try {
                manager.activeNotifications
                    .filter { it.id == id && it.tag?.startsWith(prefix) == true }
                    .sortedBy { it.postTime }
            } catch (error: RuntimeException) {
                Log.w(
                    TAG,
                    "notification_delivery category=${category.name.lowercase()} " +
                        "result=inspection_failed",
                    error,
                )
                return false
            }
        active.take((active.size - MAX_ACTIVE_PER_CATEGORY + 1).coerceAtLeast(0))
            .forEach { manager.cancel(it.tag, it.id) }
        return true
    }

    private fun cancelTorrentNotifications(torrentId: String) {
        ProductNotificationCategory.entries.forEach { category ->
            manager.cancel(
                productNotificationTag(category, torrentId),
                eventNotificationId(category),
            )
        }
    }

    private fun platformState(interactionLeaseCount: Int): ProductNotificationState {
        val permissionGranted =
            Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
                context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) ==
                PackageManager.PERMISSION_GRANTED
        val appEnabled = manager.areNotificationsEnabled()
        return ProductNotificationState(
            preferences = preferences,
            permissionGranted = permissionGranted,
            appNotificationsEnabled = appEnabled,
            backgroundChannelEnabled = channelEnabled(AndroidNotificationContract.BACKGROUND_CHANNEL_ID),
            completionChannelEnabled = channelEnabled(AndroidNotificationContract.COMPLETION_CHANNEL_ID),
            attentionChannelEnabled = channelEnabled(AndroidNotificationContract.ATTENTION_CHANNEL_ID),
            interactionLeaseCount = interactionLeaseCount,
            preferenceError = state.value.notifications.preferenceError,
        )
    }

    private fun channelEnabled(channelId: String): Boolean =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.O ||
            manager.getNotificationChannel(channelId)?.importance?.let {
                it != NotificationManager.IMPORTANCE_NONE
            } == true

    private fun eventNotificationId(category: ProductNotificationCategory): Int =
        when (category) {
            ProductNotificationCategory.DOWNLOAD_COMPLETE -> COMPLETION_NOTIFICATION_ID
            ProductNotificationCategory.NEEDS_ATTENTION -> ATTENTION_NOTIFICATION_ID
        }

    private fun productNotificationTagPrefix(category: ProductNotificationCategory): String =
        "rstorrent-${category.name.lowercase()}-"

    companion object {
        private const val ONGOING_OPEN_REQUEST = 42
        private const val ONGOING_STOP_REQUEST = 43
        private const val COMPLETION_NOTIFICATION_ID = 44
        private const val ATTENTION_NOTIFICATION_ID = 45
        private const val MAX_ACTIVE_PER_CATEGORY = 32
        private const val TAG = "RSTorrentProduct"
    }
}

internal fun productOngoingNotificationText(product: ProductState): String {
    if (!product.ready && product.error == null) return "Opening profile"
    if (product.error != null) return "RSTorrent needs attention"
    val downloading = product.torrents.values.count { it.state == TorrentState.DOWNLOADING }
    if (downloading == 1) return "Downloading 1 torrent"
    if (downloading > 1) return "Downloading $downloading torrents"
    if (product.companionPort != null) return "Ready for Chrome"
    return "Ready"
}
