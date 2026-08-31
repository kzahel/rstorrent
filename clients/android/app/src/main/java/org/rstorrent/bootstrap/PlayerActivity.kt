@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)
@file:androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)

package org.rstorrent.bootstrap

import android.app.PictureInPictureParams
import android.content.Context
import android.content.Intent
import android.content.res.Configuration
import android.graphics.Rect
import android.os.Build
import android.os.Bundle
import android.util.Rational
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.viewinterop.AndroidView
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.VideoSize
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.ui.PlayerView
import kotlin.math.roundToInt
import org.rstorrent.bootstrap.ui.ProductThemeMode
import org.rstorrent.bootstrap.ui.RstorrentTheme

class PlayerActivity : ComponentActivity() {
    private var player: ExoPlayer? = null
    private var playerView: PlayerView? = null
    private var playbackTitle by mutableStateOf("Media")
    private var preparing by mutableStateOf(true)
    private var playbackError by mutableStateOf<String?>(null)
    private var inPictureInPicture by mutableStateOf(false)
    private var interactionLeaseHeld = false
    private var videoAspectRatio: Rational? = null

    private val playerListener =
        object : Player.Listener {
            override fun onPlaybackStateChanged(playbackState: Int) {
                preparing =
                    playbackState == Player.STATE_IDLE || playbackState == Player.STATE_BUFFERING
                updatePictureInPictureParameters()
            }

            override fun onPlayerError(error: PlaybackException) {
                preparing = false
                playbackError = error.localizedMessage ?: "Playback failed"
                updatePictureInPictureParameters()
            }

            override fun onVideoSizeChanged(videoSize: VideoSize) {
                if (videoSize.width <= 0 || videoSize.height <= 0) return
                val ratio =
                    (videoSize.width.toDouble() * videoSize.pixelWidthHeightRatio) /
                        videoSize.height.toDouble()
                val bounded = ratio.coerceIn(MIN_PIP_ASPECT_RATIO, MAX_PIP_ASPECT_RATIO)
                videoAspectRatio = Rational((bounded * ASPECT_RATIO_SCALE).roundToInt(), ASPECT_RATIO_SCALE)
                updatePictureInPictureParameters()
            }
        }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val request = consumeRequest(intent)
        if (request == null) {
            finish()
            return
        }
        acquireInteractionLease()
        initializePlayer(request)
        setContent {
            RstorrentTheme(ProductThemeMode.SYSTEM, dynamicColor = true) {
                if (inPictureInPicture) {
                    PlayerSurface()
                } else {
                    Scaffold(
                        topBar = {
                            TopAppBar(
                                navigationIcon = {
                                    IconButton(onClick = ::finish) {
                                        Icon(
                                            Icons.AutoMirrored.Outlined.ArrowBack,
                                            contentDescription = "Close player",
                                        )
                                    }
                                },
                                title = { Text(playbackTitle, maxLines = 1) },
                            )
                        },
                    ) { padding ->
                        Box(
                            Modifier
                                .fillMaxSize()
                                .padding(padding)
                                .background(Color.Black),
                        ) {
                            PlayerSurface()
                            if (preparing && playbackError == null) {
                                CircularProgressIndicator(Modifier.align(Alignment.Center))
                            }
                            playbackError?.let { message ->
                                Text(
                                    message,
                                    color = MaterialTheme.colorScheme.error,
                                    modifier = Modifier.align(Alignment.Center),
                                )
                            }
                        }
                    }
                }
            }
        }
    }

    @androidx.compose.runtime.Composable
    private fun PlayerSurface() {
        AndroidView(
            modifier = Modifier.fillMaxSize(),
            factory = { context ->
                PlayerView(context).apply {
                    player = this@PlayerActivity.player
                    useController = !inPictureInPicture
                    setShowBuffering(PlayerView.SHOW_BUFFERING_ALWAYS)
                    playerView = this
                }
            },
            update = { view ->
                view.player = player
                view.useController = !inPictureInPicture
                playerView = view
            },
        )
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        val request = consumeRequest(intent)
        if (request == null) {
            player?.stop()
            playbackError = "Invalid playback request"
            preparing = false
            updatePictureInPictureParameters()
            return
        }
        playbackTitle = request.title
        playbackError = null
        preparing = true
        player?.apply {
            setMediaItem(MediaItem.fromUri(request.source))
            prepare()
            playWhenReady = true
        }
    }

    override fun onUserLeaveHint() {
        super.onUserLeaveHint()
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S && canEnterPictureInPicture()) {
            runCatching { enterPictureInPictureMode(currentPictureInPictureParameters()) }
        }
    }

    override fun onPictureInPictureModeChanged(
        isInPictureInPictureMode: Boolean,
        newConfig: Configuration,
    ) {
        super.onPictureInPictureModeChanged(isInPictureInPictureMode, newConfig)
        inPictureInPicture = isInPictureInPictureMode
    }

    override fun onDestroy() {
        playerView?.player = null
        playerView = null
        player?.removeListener(playerListener)
        player?.release()
        player = null
        releaseInteractionLease()
        super.onDestroy()
    }

    private fun initializePlayer(request: PlaybackRequest) {
        playbackTitle = request.title
        val audioAttributes =
            AudioAttributes.Builder()
                .setUsage(C.USAGE_MEDIA)
                .setContentType(C.AUDIO_CONTENT_TYPE_MOVIE)
                .build()
        player =
            ExoPlayer.Builder(this)
                .setAudioAttributes(audioAttributes, true)
                .build()
                .also { mediaPlayer ->
                    mediaPlayer.addListener(playerListener)
                    mediaPlayer.setMediaItem(MediaItem.fromUri(request.source))
                    mediaPlayer.prepare()
                    mediaPlayer.playWhenReady = true
                }
        updatePictureInPictureParameters()
    }

    private fun acquireInteractionLease() {
        if (interactionLeaseHeld) return
        interactionLeaseHeld = true
        ProductInteractionRegistry.setLease(ProductEngineService.INTERACTION_PLAYBACK, true)
    }

    private fun releaseInteractionLease() {
        if (!interactionLeaseHeld) return
        interactionLeaseHeld = false
        ProductInteractionRegistry.setLease(ProductEngineService.INTERACTION_PLAYBACK, false)
    }

    private fun canEnterPictureInPicture(): Boolean =
        !isFinishing && player != null && playbackError == null

    private fun currentPictureInPictureParameters(): PictureInPictureParams {
        val builder = PictureInPictureParams.Builder()
        videoAspectRatio?.let(builder::setAspectRatio)
        val sourceRect = Rect()
        if (playerView?.getGlobalVisibleRect(sourceRect) == true && !sourceRect.isEmpty) {
            builder.setSourceRectHint(sourceRect)
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            builder.setAutoEnterEnabled(canEnterPictureInPicture())
            builder.setSeamlessResizeEnabled(true)
        }
        return builder.build()
    }

    private fun updatePictureInPictureParameters() {
        if (isFinishing) return
        runCatching { setPictureInPictureParams(currentPictureInPictureParameters()) }
    }

    private fun consumeRequest(intent: Intent): PlaybackRequest? {
        val source = intent.getStringExtra(EXTRA_SOURCE)
        val title = intent.getStringExtra(EXTRA_TITLE)?.take(MAX_TITLE_CHARACTERS)
        intent.removeExtra(EXTRA_SOURCE)
        intent.removeExtra(EXTRA_TITLE)
        if (source == null) return null
        return runCatching {
            PlaybackRequest(
                AndroidMediaPlaybackPolicy.requireCapabilityUrl(source),
                title?.takeIf(String::isNotBlank) ?: "Media",
            )
        }.getOrNull()
    }

    private data class PlaybackRequest(
        val source: String,
        val title: String,
    )

    companion object {
        private const val EXTRA_SOURCE = "org.rstorrent.bootstrap.extra.MEDIA_SOURCE"
        private const val EXTRA_TITLE = "org.rstorrent.bootstrap.extra.MEDIA_TITLE"
        private const val MAX_TITLE_CHARACTERS = 256
        private const val ASPECT_RATIO_SCALE = 10_000
        private const val MIN_PIP_ASPECT_RATIO = 1.0 / 2.39
        private const val MAX_PIP_ASPECT_RATIO = 2.39

        internal fun launchIntent(
            context: Context,
            source: String,
            title: String,
        ): Intent =
            Intent(context, PlayerActivity::class.java).apply {
                putExtra(EXTRA_SOURCE, AndroidMediaPlaybackPolicy.requireCapabilityUrl(source))
                putExtra(EXTRA_TITLE, title.take(MAX_TITLE_CHARACTERS))
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP)
                addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP)
            }
    }
}
